// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Infra adapter for explicit operator actions. Never starts a server or worker.
use std::time::{SystemTime, UNIX_EPOCH};

use nilxone_identity::{
    IdentityRepository, PubDress,
    repository::{
        CreateBusinessOrganizationOutcome, OperatorBondKind, OperatorMemberRole,
        OperatorMembershipOutcome,
    },
};

enum Command {
    List(OperatorBondKind),
    Create { business: PubDress, owner: PubDress },
    Add { business: PubDress, member: PubDress, role: OperatorMemberRole },
}

fn parse(words: &[String]) -> Result<Command, String> {
    let args: Vec<&str> = words.iter().map(String::as_str).collect();
    let address = |value: &str| value.parse::<PubDress>().map_err(|_| "invalid pub_dress".to_owned());
    match args.as_slice() {
        ["list", "bonds"] => Ok(Command::List(OperatorBondKind::Human)),
        ["list", "business-bonds"] => Ok(Command::List(OperatorBondKind::Business)),
        ["create-business", business, owner] => Ok(Command::Create {
            business: address(business)?, owner: address(owner)?,
        }),
        ["add-member", business, member, role] => Ok(Command::Add {
            business: address(business)?, member: address(member)?,
            role: match *role {
                "member" => OperatorMemberRole::Member,
                "owner" => OperatorMemberRole::Owner,
                _ => return Err("role must be member or owner".into()),
            },
        }),
        _ => Err("usage: list bonds|business-bonds; create-business BUSINESS OWNER; add-member BUSINESS MEMBER member|owner".into()),
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Parse BEFORE opening a database: missing owner, unknown role, or an
    // unsupported lifecycle command must never cause a database side effect.
    let mut words = std::env::args().skip(1).collect::<Vec<_>>();
    let validate = words.first().is_some_and(|word| word == "--validate");
    if validate {
        words.remove(0);
    }
    let command = parse(&words)?;
    if validate {
        println!("{}", serde_json::json!({"schema": 1, "args": words}));
        return Ok(());
    }
    let url = std::env::var("DATABASE_URL")?;
    let read_only = matches!(&command, Command::List(_));
    let repo = IdentityRepository::connect_operator(&url, read_only).await?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let result = match command {
        Command::List(kind) => {
            serde_json::json!({"bonds": repo.operator_bonds(kind).await?})
        }
        Command::Create { business, owner } => {
            let status = match repo.operator_create_business(&business, &owner, now).await? {
                CreateBusinessOrganizationOutcome::Created => "created",
                CreateBusinessOrganizationOutcome::IdempotentReplay => "already_owned",
                other => return Err(format!("business creation refused: {other:?}").into()),
            };
            serde_json::json!({"status": status, "business": business.as_str(),
                "owner": owner.as_str(), "role": "owner"})
        }
        Command::Add { business, member, role } => {
            let status = match repo.operator_add_member(&business, &member, role, now).await? {
                OperatorMembershipOutcome::Added => "added",
                OperatorMembershipOutcome::Promoted => "promoted",
                other => return Err(format!("membership refused: {other:?}").into()),
            };
            serde_json::json!({"status": status, "business": business.as_str(),
                "member": member.as_str(), "role": role.as_str()})
        }
    };
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("identity operator: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_is_required_and_unagreed_lifecycle_operations_are_closed() {
        for line in [
            "create-business 0xdstudio",
            "create-business 0xdstudio shell;injection",
            "add-member 0xdstudio 0x1alice maintainer",
            "hire 0xdstudio 0x1alice",
            "freed 0xdstudio 0x1alice",
            "delete 0xdstudio",
        ] {
            assert!(parse(&line.split_whitespace().map(String::from).collect::<Vec<_>>()).is_err());
        }
    }
}
