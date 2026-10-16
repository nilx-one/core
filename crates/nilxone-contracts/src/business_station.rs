// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! The address of a business station: the stationed Avaia that stands at one
//! of a business Bond's shops.
//!
//! A station is named after its business, not as a companion Avaia: the
//! business's own full `PubDress`, `-`, then a place, with no `ai` ending
//! (`0xda-market-zoloti-vorota`). The whole address is a valid `PubDress`, so
//! it shares the one global namespace with every Bond.
//!
//! A business names a station by a caption, kept as written by whoever stores
//! it. The address is formed from the caption once: a leading business name
//! (`0xda-market` or `xda-market`) is the prefix already and is not repeated;
//! the place is transliterated (Ukrainian by the official 2010 national
//! table), lowercased, and whitespace becomes `-`.

use core::{fmt, str::FromStr};

use crate::pub_dress::{PubDress, PubDressError};

/// The longest caption accepted, in Unicode scalars. The address it forms is
/// bounded again by the `PubDress` grammar.
pub const MAX_STATION_CAPTION_SCALARS: usize = 64;

/// A business station's address, checked against the business that keeps it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StationPubDress {
    address: PubDress,
    owner: PubDress,
}

/// A caption as a business wrote it, and the station address it forms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StationCaption {
    /// The caption, trimmed, with inner whitespace runs collapsed.
    pub caption: String,
    pub address: StationPubDress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StationPubDressError {
    /// The caption is empty.
    EmptyCaption,
    /// The caption is longer than [`MAX_STATION_CAPTION_SCALARS`].
    CaptionTooLong,
    /// The caption names the business and no place.
    NoPlace,
    /// The address is not this business's own name, `-`, then a place.
    NotOwnersAddress,
    /// The formed or given address is not a `PubDress`.
    Address(PubDressError),
}

impl StationPubDressError {
    /// Returns the binding-safe failure code.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::EmptyCaption => "empty_caption",
            Self::CaptionTooLong => "caption_too_long",
            Self::NoPlace => "no_place",
            Self::NotOwnersAddress => "not_owners_address",
            Self::Address(error) => error.code(),
        }
    }
}

impl fmt::Display for StationPubDressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCaption => formatter.write_str("the caption is empty"),
            Self::CaptionTooLong => write!(
                formatter,
                "the caption is longer than {MAX_STATION_CAPTION_SCALARS} characters"
            ),
            Self::NoPlace => formatter.write_str("the caption names no place"),
            Self::NotOwnersAddress => {
                formatter.write_str("the address is not its business's name and a place")
            }
            Self::Address(error) => write!(formatter, "not a pub_dress: {error}"),
        }
    }
}

impl std::error::Error for StationPubDressError {}

impl StationPubDress {
    /// Reads `value` as a station of `owner`.
    ///
    /// # Errors
    ///
    /// [`StationPubDressError::Address`] when `value` is not a `PubDress`, and
    /// [`StationPubDressError::NotOwnersAddress`] when it is not `owner`'s name,
    /// `-`, then a place.
    pub fn for_owner(owner: &PubDress, value: &str) -> Result<Self, StationPubDressError> {
        let address = PubDress::from_str(value).map_err(StationPubDressError::Address)?;
        let is_station = address
            .as_str()
            .strip_prefix(owner.as_str())
            .is_some_and(|place| place.len() > 1 && place.starts_with('-'));
        if !is_station {
            return Err(StationPubDressError::NotOwnersAddress);
        }
        Ok(Self {
            address,
            owner: owner.clone(),
        })
    }

    /// Forms the station address a business's caption names.
    ///
    /// # Errors
    ///
    /// The caption's own failures ([`StationPubDressError::EmptyCaption`],
    /// [`StationPubDressError::CaptionTooLong`], [`StationPubDressError::NoPlace`])
    /// and [`StationPubDressError::Address`] when the formed address breaks the
    /// `PubDress` grammar.
    pub fn from_caption(
        owner: &PubDress,
        caption: &str,
    ) -> Result<StationCaption, StationPubDressError> {
        let caption = normalize_caption(caption)?;
        let owner_name = owner.as_str();
        // `0xda-market` is also written `xda-market`, the way its Avaia is.
        let short_name = owner_name[1..].to_lowercase();
        let mut words = caption.split(' ').peekable();
        if words
            .peek()
            .is_some_and(|first| *first == owner_name || first.to_lowercase() == short_name)
        {
            words.next();
        }
        let place: Vec<&str> = words.collect();
        if place.is_empty() {
            return Err(StationPubDressError::NoPlace);
        }

        let mut slug = String::with_capacity(caption.len());
        for (index, word) in place.iter().enumerate() {
            if index > 0 {
                slug.push('-');
            }
            transliterate_word(word, &mut slug);
        }
        let address = Self::for_owner(owner, &format!("{owner_name}-{slug}"))?;
        Ok(StationCaption { caption, address })
    }

    /// Returns the exact canonical address.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.address.as_str()
    }

    /// Returns the address as the `PubDress` it is.
    #[must_use]
    pub fn as_pub_dress(&self) -> &PubDress {
        &self.address
    }

    /// Returns the business that keeps this station.
    #[must_use]
    pub fn owner(&self) -> &PubDress {
        &self.owner
    }
}

impl fmt::Display for StationPubDress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A caption as stored: trimmed, with inner whitespace runs collapsed.
///
/// # Errors
///
/// [`StationPubDressError::EmptyCaption`] and
/// [`StationPubDressError::CaptionTooLong`].
pub fn normalize_station_caption(caption: &str) -> Result<String, StationPubDressError> {
    normalize_caption(caption)
}

fn normalize_caption(caption: &str) -> Result<String, StationPubDressError> {
    let words: Vec<&str> = caption.split_whitespace().collect();
    if words.is_empty() {
        return Err(StationPubDressError::EmptyCaption);
    }
    let caption = words.join(" ");
    if caption.chars().count() > MAX_STATION_CAPTION_SCALARS {
        return Err(StationPubDressError::CaptionTooLong);
    }
    Ok(caption)
}

fn transliterate_word(word: &str, slug: &mut String) {
    let mut word_start = true;
    let mut previous: Option<char> = None;
    for scalar in word.chars() {
        let lower = scalar.to_lowercase().next().unwrap_or(scalar);
        match transliterate(lower, previous, word_start) {
            Some(latin) => slug.push_str(latin),
            None => slug.push(lower),
        }
        word_start = !lower.is_alphanumeric() && !is_apostrophe(lower);
        previous = Some(lower);
    }
}

fn is_apostrophe(scalar: char) -> bool {
    matches!(scalar, '\'' | 'ʼ' | '’')
}

/// Ukrainian national transliteration (Cabinet of Ministers resolution 55,
/// 2010). `None` keeps the scalar as it is.
fn transliterate(scalar: char, previous: Option<char>, word_start: bool) -> Option<&'static str> {
    Some(match scalar {
        'а' => "a",
        'б' => "b",
        'в' => "v",
        // `зг` is written `zgh`, to tell it from `ж`.
        'г' if previous == Some('з') => "gh",
        'г' => "h",
        'ґ' => "g",
        'д' => "d",
        'е' => "e",
        'є' if word_start => "ye",
        'є' => "ie",
        'ж' => "zh",
        'з' => "z",
        'и' => "y",
        'ї' if word_start => "yi",
        'й' if word_start => "y",
        'і' | 'ї' | 'й' => "i",
        'к' => "k",
        'л' => "l",
        'м' => "m",
        'н' => "n",
        'о' => "o",
        'п' => "p",
        'р' => "r",
        'с' => "s",
        'т' => "t",
        'у' => "u",
        'ф' => "f",
        'х' => "kh",
        'ц' => "ts",
        'ч' => "ch",
        'ш' => "sh",
        'щ' => "shch",
        'ь' | '\'' | 'ʼ' | '’' => "",
        'ю' if word_start => "yu",
        'ю' => "iu",
        'я' if word_start => "ya",
        'я' => "ia",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{StationPubDress, StationPubDressError, normalize_station_caption};
    use crate::{PubDress, PubDressError};

    fn market() -> PubDress {
        "0xda-market".parse().expect("owner")
    }

    #[test]
    fn forms_the_shop_addresses_from_their_captions() {
        for (caption, address) in [
            ("xda-market Золоті Ворота", "0xda-market-zoloti-vorota"),
            (
                "0xda-market Батьківщина-Мати",
                "0xda-market-batkivshchyna-maty",
            ),
            ("Батьківщина-Мати", "0xda-market-batkivshchyna-maty"),
            ("xda-market  Згорани ", "0xda-market-zghorany"),
            ("Їжак Юрій Яр", "0xda-market-yizhak-yurii-yar"),
            ("В'ячеслав", "0xda-market-viacheslav"),
            ("Поділ 2", "0xda-market-podil-2"),
        ] {
            let formed = StationPubDress::from_caption(&market(), caption)
                .map(|station| station.address.to_string());
            assert_eq!(formed, Ok(address.to_owned()), "{caption}");
        }
    }

    #[test]
    fn keeps_the_caption_as_written() {
        let formed = StationPubDress::from_caption(&market(), "  xda-market   Золоті Ворота ")
            .expect("caption");
        assert_eq!(formed.caption, "xda-market Золоті Ворота");
        assert_eq!(formed.address.owner(), &market());
        assert_eq!(
            normalize_station_caption("   "),
            Err(StationPubDressError::EmptyCaption)
        );
        assert_eq!(
            normalize_station_caption(&"я".repeat(65)),
            Err(StationPubDressError::CaptionTooLong)
        );
    }

    #[test]
    fn refuses_a_caption_that_forms_no_station_address() {
        assert_eq!(
            StationPubDress::from_caption(&market(), "xda-market"),
            Err(StationPubDressError::NoPlace)
        );
        // The PubDress grammar bounds the address, whatever the caption allows.
        assert_eq!(
            StationPubDress::from_caption(&market(), "Їжакевич Юрій Яготинський"),
            Err(StationPubDressError::Address(PubDressError::InvalidLength))
        );
        assert_eq!(
            StationPubDress::from_caption(&market(), "🌍"),
            Err(StationPubDressError::Address(
                PubDressError::InvalidCharacter
            ))
        );
    }

    #[test]
    fn a_station_address_is_its_business_then_a_place() {
        let station =
            StationPubDress::for_owner(&market(), "0xda-market-zoloti-vorota").expect("station");
        assert_eq!(station.as_str(), "0xda-market-zoloti-vorota");
        for other in [
            "0xda-market",
            "0xda-market-",
            "0xda-marketing",
            "0xdrival-shop",
        ] {
            assert!(
                StationPubDress::for_owner(&market(), other).is_err(),
                "{other}"
            );
        }
        assert_eq!(
            StationPubDress::for_owner(&market(), "xda-market-shop"),
            Err(StationPubDressError::Address(PubDressError::InvalidPrefix))
        );
    }

    #[test]
    fn failure_codes_are_stable() {
        assert_eq!(StationPubDressError::NoPlace.code(), "no_place");
        assert_eq!(
            StationPubDressError::NotOwnersAddress.code(),
            "not_owners_address"
        );
        assert_eq!(
            StationPubDressError::Address(PubDressError::InvalidLength).code(),
            PubDressError::InvalidLength.code()
        );
    }
}
