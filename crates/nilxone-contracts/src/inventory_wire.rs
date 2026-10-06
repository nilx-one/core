// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! The wire form of finds, the economy and an [`Inventory`], for bindings.
//!
//! A host stores the inventory as canonical JSON and sends one command at a
//! time; Core answers with the next state and what it paid. Every u64 travels
//! as a decimal string, as elsewhere in the contract. Unknown members are
//! refused, and a stored state is checked as it is read: things that overlap
//! or fall outside their grid are refused, never repaired.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::backpack::{Backpack, Carry, size_of};
use crate::economy::{CRAFTED, EconomyError, Place, RECIPES, item_kind, recipe};
use crate::find_item::{CATALOG, FindTier, PickupRarities, item_for_find};
use crate::inventory::{Holder, Inventory, Outcome};
use crate::{DecimalU64, ECONOMY_VERSION, FIND_CATALOG_VERSION, SEED_CODE, SEED_EMBLEM};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThingWire {
    id: String,
    x: u8,
    y: u8,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BackpackWire {
    carry: String,
    things: Vec<ThingWire>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CraftWire {
    recipe: String,
    started_ms: DecimalU64,
    ready_ms: DecimalU64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InventoryWire {
    bond: BackpackWire,
    avaia: BackpackWire,
    seeds: DecimalU64,
    craft: Option<CraftWire>,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    PickUp {
        holder: String,
        artifact_id: String,
        tier: u8,
    },
    Rearrange {
        holder: String,
        from: (u8, u8),
        to: (u8, u8),
    },
    HandOver {
        from: String,
        x: u8,
        y: u8,
    },
    SwitchCarry {
        holder: String,
        carry: String,
    },
    Sell {
        id: String,
        count: u32,
    },
    StartCraft {
        recipe: String,
        place: String,
    },
    FinishCraft,
    FinishPaid,
}

/// Failure reading the wire, on top of [`EconomyError`].
const INVALID: &str = "invalid";

fn holder(code: &str) -> Result<Holder, &'static str> {
    match code {
        "bond" => Ok(Holder::Bond),
        "avaia" => Ok(Holder::Avaia),
        _ => Err(INVALID),
    }
}

fn place(code: &str) -> Result<Place, &'static str> {
    [Place::Anywhere, Place::RepairWorkshop]
        .into_iter()
        .find(|place| place.code() == code)
        .ok_or(INVALID)
}

fn backpack_from_wire(wire: &BackpackWire) -> Result<Backpack, &'static str> {
    let mut backpack = Backpack::new(Carry::from_code(&wire.carry).ok_or(INVALID)?);
    for thing in &wire.things {
        backpack
            .put_at(&thing.id, thing.x, thing.y)
            .map_err(|_| INVALID)?;
    }
    Ok(backpack)
}

fn backpack_to_wire(backpack: &Backpack) -> BackpackWire {
    BackpackWire {
        carry: backpack.carry().code().to_owned(),
        things: backpack
            .placed()
            .iter()
            .map(|placed| ThingWire {
                id: placed.id.to_owned(),
                x: placed.x,
                y: placed.y,
            })
            .collect(),
    }
}

/// Reads a stored inventory.
fn read_state(state: &str) -> Result<Inventory, &'static str> {
    if state.is_empty() {
        return Ok(Inventory::default());
    }
    let wire: InventoryWire = serde_json::from_str(state).map_err(|_| INVALID)?;
    let mut inventory = Inventory::restore(
        backpack_from_wire(&wire.bond)?,
        backpack_from_wire(&wire.avaia)?,
        wire.seeds.get(),
    );
    if let Some(craft) = wire.craft {
        inventory
            .resume_craft(&craft.recipe, craft.started_ms.get(), craft.ready_ms.get())
            .map_err(|_| INVALID)?;
    }
    Ok(inventory)
}

fn write_state(inventory: &Inventory) -> Value {
    let wire = InventoryWire {
        bond: backpack_to_wire(inventory.backpack(Holder::Bond)),
        avaia: backpack_to_wire(inventory.backpack(Holder::Avaia)),
        seeds: DecimalU64::new(inventory.seeds()),
        craft: inventory.craft_job().map(|job| CraftWire {
            recipe: job.recipe.id.to_owned(),
            started_ms: DecimalU64::new(job.started_ms),
            ready_ms: DecimalU64::new(job.ready_ms),
        }),
    };
    serde_json::to_value(wire).unwrap_or(Value::Null)
}

fn failure(code: &str) -> String {
    json!({ "ok": false, "error": code }).to_string()
}

fn outcome_value(outcome: Outcome) -> Value {
    json!({
        "seeds_gained": DecimalU64::new(outcome.seeds_gained),
        "seeds_spent": DecimalU64::new(outcome.seeds_spent),
        "experience": outcome.experience,
    })
}

/// Applies one command to a stored inventory at `now_ms`.
///
/// `state` is the stored JSON, or the empty string for a new inventory.
/// The answer is `{"ok":true,"state":…,"outcome":…}` with the next state to
/// store, plus `"item"` for a pick-up, or `{"ok":false,"error":"<code>"}`
/// with an [`EconomyError`] code or `invalid` for a malformed state or
/// command. A refused command leaves the stored state as it was.
#[must_use]
pub fn apply_inventory_command(state: &str, command: &str, now_ms: u64) -> String {
    let mut inventory = match read_state(state) {
        Ok(inventory) => inventory,
        Err(code) => return failure(code),
    };
    let Ok(command) = serde_json::from_str::<Command>(command) else {
        return failure(INVALID);
    };
    let mut item = None;
    let result: Result<Outcome, Result<EconomyError, &'static str>> = (|| match command {
        Command::PickUp {
            holder: who,
            artifact_id,
            tier,
        } => {
            let tier = FindTier::new(tier).ok_or(Err(INVALID))?;
            let found = item_for_find(&artifact_id, tier).map_err(|_| Err(INVALID))?;
            item = Some(found.id);
            inventory
                .pick_up(holder(&who).map_err(Err)?, found)
                .map_err(Ok)
        }
        Command::Rearrange {
            holder: who,
            from,
            to,
        } => inventory
            .rearrange(holder(&who).map_err(Err)?, from, to)
            .map(|_| Outcome::default())
            .map_err(Ok),
        Command::HandOver { from, x, y } => inventory
            .hand_over(holder(&from).map_err(Err)?, x, y)
            .map(|_| Outcome::default())
            .map_err(Ok),
        Command::SwitchCarry { holder: who, carry } => inventory
            .switch_carry(
                holder(&who).map_err(Err)?,
                Carry::from_code(&carry).ok_or(Err(INVALID))?,
            )
            .map(|()| Outcome::default())
            .map_err(Ok),
        Command::Sell { id, count } => inventory.sell(&id, count).map_err(Ok),
        Command::StartCraft {
            recipe: id,
            place: at,
        } => {
            let recipe = recipe(&id).ok_or(Ok(EconomyError::UnknownItem))?;
            inventory
                .start_craft(recipe, place(&at).map_err(Err)?, now_ms)
                .map(|_| Outcome::default())
                .map_err(Ok)
        }
        Command::FinishCraft => inventory.finish_craft(now_ms).map_err(Ok),
        Command::FinishPaid => inventory.finish_paid().map_err(Ok),
    })();
    match result {
        Ok(outcome) => {
            let mut answer = json!({
                "ok": true,
                "state": write_state(&inventory),
                "outcome": outcome_value(outcome),
            });
            if let Some(item) = item {
                answer["item"] = json!(item);
            }
            answer.to_string()
        }
        Err(Ok(error)) => failure(error.code()),
        Err(Err(code)) => failure(code),
    }
}

/// Which item a find is: `item:<id>` or `error:<code>`.
#[must_use]
pub fn find_item_wire(artifact_id: &str, tier: u8) -> String {
    let Some(tier) = FindTier::new(tier) else {
        return "error:tier".to_owned();
    };
    match item_for_find(artifact_id, tier) {
        Ok(found) => format!("item:{}", found.id),
        Err(error) => format!("error:{}", error.code()),
    }
}

/// Whether a find of `tier` is picked up under the stored pick-up setting:
/// `yes`, `no`, or `error:<code>`.
#[must_use]
pub fn picks_up_wire(rarities: &str, tier: u8) -> String {
    let Some(tier) = FindTier::new(tier) else {
        return "error:tier".to_owned();
    };
    match PickupRarities::from_wire(rarities) {
        Ok(rarities) if rarities.picks_up(tier) => "yes".to_owned(),
        Ok(_) => "no".to_owned(),
        Err(error) => format!("error:{}", error.code()),
    }
}

/// Everything a host needs to show the economy: things, recipes, grids and
/// the currency, as canonical JSON.
#[must_use]
pub fn economy_catalog_json() -> String {
    let size =
        |id: &str| size_of(id).map(|size| json!({ "width": size.width, "height": size.height }));
    let price = |id: &str| {
        item_kind(id)
            .and_then(|kind| kind.sell_price)
            .map(DecimalU64::new)
    };
    let found: Vec<Value> = CATALOG
        .iter()
        .map(|found| {
            json!({
                "id": found.id,
                "tier": found.tier.get(),
                "rarity": found.tier.rarity().code(),
                "experience": found.experience,
                "seeds": DecimalU64::new(found.seeds),
                "condition": found.condition.code(),
                "city": found.city.map(crate::find_item::City::code),
                "sell_price": price(found.id),
                "size": size(found.id),
            })
        })
        .collect();
    let crafted: Vec<Value> = CRAFTED
        .iter()
        .map(|kind| {
            json!({
                "id": kind.id,
                "sell_price": kind.sell_price.map(DecimalU64::new),
                "size": size(kind.id),
            })
        })
        .collect();
    let recipes: Vec<Value> = RECIPES
        .iter()
        .map(|recipe| {
            json!({
                "id": recipe.id,
                "consumes": recipe.consumes.iter().map(|(id, count)| json!({ "id": id, "count": count })).collect::<Vec<_>>(),
                "tools": recipe.tools,
                "seeds": DecimalU64::new(recipe.seeds),
                "makes": recipe.makes,
                "experience": recipe.experience,
                "place": recipe.place.code(),
                "minutes": recipe.minutes,
                "legendary": recipe.legendary,
            })
        })
        .collect();
    let carries: Vec<Value> = Carry::ALL
        .iter()
        .map(|carry| {
            let grid = carry.grid();
            json!({ "id": carry.code(), "width": grid.width, "height": grid.height })
        })
        .collect();
    json!({
        "find_catalog_version": FIND_CATALOG_VERSION,
        "economy_version": ECONOMY_VERSION,
        "currency": { "code": SEED_CODE, "emblem": SEED_EMBLEM },
        "found": found,
        "crafted": crafted,
        "recipes": recipes,
        "carries": carries,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::{apply_inventory_command, economy_catalog_json, find_item_wire, picks_up_wire};
    use serde_json::Value;

    const KYIV: &str = "art:seg:312346:298243:e2908:1:0";

    fn apply(state: &str, command: &str, now_ms: u64) -> Value {
        serde_json::from_str(&apply_inventory_command(state, command, now_ms)).unwrap()
    }

    #[test]
    fn a_find_names_its_item() {
        assert_eq!(find_item_wire(KYIV, 1), "item:bottle_cap");
        assert_eq!(find_item_wire(KYIV, 7), "error:tier");
        assert_eq!(find_item_wire("art:x", 1), "error:artifact_id");
        assert_eq!(picks_up_wire("rare", 5), "yes");
        assert_eq!(picks_up_wire("rare", 1), "no");
        assert_eq!(picks_up_wire("epic", 1), "error:pickup_rarities");
    }

    #[test]
    fn a_new_inventory_picks_up_and_round_trips() {
        let picked = apply(
            "",
            &format!(r#"{{"op":"pick_up","holder":"avaia","artifact_id":"{KYIV}","tier":1}}"#),
            0,
        );
        assert_eq!(picked["ok"], true);
        assert_eq!(picked["item"], "bottle_cap");
        let state = picked["state"].to_string();
        assert_eq!(
            state,
            r#"{"avaia":{"carry":"backpack","things":[{"id":"bottle_cap","x":0,"y":0}]},"bond":{"carry":"backpack","things":[]},"craft":null,"seeds":"0"}"#
        );
        let handed = apply(
            &state,
            r#"{"op":"hand_over","from":"avaia","x":0,"y":0}"#,
            0,
        );
        let sold = apply(
            &handed["state"].to_string(),
            r#"{"op":"sell","id":"bottle_cap","count":1}"#,
            0,
        );
        assert_eq!(sold["outcome"]["seeds_gained"], "2");
        assert_eq!(sold["state"]["seeds"], "2");
    }

    #[test]
    fn a_craft_runs_through_the_wire() {
        let state = r#"{"bond":{"carry":"backpack","things":[{"id":"broken_cd_player","x":0,"y":0}]},"avaia":{"carry":"pocket","things":[]},"seeds":"250","craft":null}"#;
        let started = apply(
            state,
            r#"{"op":"start_craft","recipe":"repair_cd_player","place":"repair_workshop"}"#,
            1_000,
        );
        assert_eq!(started["state"]["craft"]["ready_ms"], "2701000");
        let early = apply(
            &started["state"].to_string(),
            r#"{"op":"finish_craft"}"#,
            2_000,
        );
        assert_eq!(early["error"], "not_ready");
        let done = apply(
            &started["state"].to_string(),
            r#"{"op":"finish_craft"}"#,
            2_701_000,
        );
        assert_eq!(done["outcome"]["experience"], 50);
        assert_eq!(done["state"]["bond"]["things"][0]["id"], "cd_player");
    }

    #[test]
    fn bad_states_and_commands_are_refused() {
        let overlapping = r#"{"bond":{"carry":"pocket","things":[{"id":"can","x":0,"y":0},{"id":"can","x":0,"y":0}]},"avaia":{"carry":"pocket","things":[]},"seeds":"0","craft":null}"#;
        for (state, command) in [
            (overlapping, r#"{"op":"finish_craft"}"#),
            ("{}", r#"{"op":"finish_craft"}"#),
            ("", r#"{"op":"fly"}"#),
            ("", r#"{"op":"sell","id":"can","count":1,"extra":1}"#),
            (
                "",
                r#"{"op":"switch_carry","holder":"bond","carry":"suitcase"}"#,
            ),
        ] {
            assert_eq!(
                apply(state, command, 0)["error"],
                "invalid",
                "{state} {command}"
            );
        }
        assert_eq!(apply("", r#"{"op":"finish_craft"}"#, 0)["error"], "no_job");
        assert_eq!(
            apply(
                "",
                r#"{"op":"start_craft","recipe":"nothing","place":"anywhere"}"#,
                0
            )["error"],
            "unknown_item"
        );
    }

    #[test]
    fn the_catalog_lists_everything() {
        let catalog: Value = serde_json::from_str(&economy_catalog_json()).unwrap();
        assert_eq!(catalog["currency"]["emblem"], "₴€£");
        assert_eq!(catalog["carries"][2]["width"], 12);
        assert!(
            catalog["found"]
                .as_array()
                .unwrap()
                .iter()
                .all(|found| { found["seeds"] != "0" || !found["size"].is_null() })
        );
        assert!(
            catalog["recipes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|recipe| recipe["legendary"] == true)
        );
    }
}
