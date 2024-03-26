use const_format::concatcp;
use once_cell::sync::Lazy;
use poise::serenity_prelude::{Message, Timestamp};
use regex::Regex;
use std::ops::Add;
use std::time::Duration;

const DURATION_PATTERN: &str = r"(?:(\d)+d \+ )?(?:(\d+):)?(\d+):(\d+)";
static RESCUE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concatcp!(
        r"another animal in \*\*",
        DURATION_PATTERN,
        r"\*\*"
    ))
    .unwrap()
});
static RESCUE_MODIFIER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(concatcp!("finishes in ", DURATION_PATTERN)).unwrap());
static RESCUE_TODO_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concatcp!(
        r"Next Rescue: \*\*",
        DURATION_PATTERN,
        r"\*\* \(<t:(\d+)>\)"
    ))
    .unwrap()
});

static QUEST_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concatcp!(
        r"quest will finish in \*\*",
        DURATION_PATTERN,
        r"\*\*"
    ))
    .unwrap()
});
static QUEST_TODO_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concatcp!(
        r"Quest Finishes: \*\*",
        DURATION_PATTERN,
        r"\*\* \(<t:(\d+)>\)"
    ))
    .unwrap()
});

static CARD_TODO_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concatcp!(
        r"Next Card Pull: \*\*",
        DURATION_PATTERN,
        r"\*\* \(<t:(\d+)>\)"
    ))
    .unwrap()
});

static PROFILE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(concatcp!(r"change profiles in ", DURATION_PATTERN)).unwrap());

pub fn extract_rescue_cooldown(message: &Message) -> Option<Timestamp> {
    if let Some(embed) = message.embeds.first() {
        // Polar Star cosmetic
        if let Some(duration) = embed
            .fields
            .iter()
            .find(|field| field.name == "🕓 Cooldown")
            .and_then(|field| parse_duration(&field.value))
        {
            return Some(Timestamp::from(message.timestamp.add(duration)));
        }

        // info command
        if let Some(description) = &embed.description {
            if let Some(ts) = RESCUE_TODO_RE
                .captures(description)
                .and_then(|captures| captures[5].parse().ok())
                .and_then(|secs| Timestamp::from_unix_timestamp(secs).ok())
            {
                return Some(ts);
            }
        }
    }

    // Regular message
    if let Some(duration) = RESCUE_RE
        .captures(&message.content)
        .and_then(|captures| parse_duration_captures(captures))
    {
        return Some(Timestamp::from(message.timestamp.add(duration)));
    }

    // Terminal `to-do` command
    if let Some(ts) = RESCUE_TODO_RE
        .captures(&message.content)
        .and_then(|captures| captures[5].parse().ok())
        .and_then(|secs| Timestamp::from_unix_timestamp(secs).ok())
    {
        return Some(ts);
    }

    // Cooldown modifier after a rescue
    if let Some(duration) = RESCUE_MODIFIER_RE
        .captures(&message.content)
        .and_then(|captures| parse_duration_captures(captures))
    {
        return Some(Timestamp::from(message.timestamp.add(duration)));
    }

    None
}

pub fn extract_quest_cooldown(message: &Message) -> Option<Timestamp> {
    if let Some(embed) = message.embeds.first() {
        // Polar Star cosmetic
        if let Some(duration) = embed
            .fields
            .iter()
            .find(|field| field.name == "🌲 Quest ends")
            .and_then(|field| parse_duration(&field.value))
        {
            return Some(Timestamp::from(message.timestamp.add(duration)));
        }

        // info command
        if let Some(description) = &embed.description {
            if let Some(ts) = QUEST_TODO_RE
                .captures(description)
                .and_then(|captures| captures[5].parse().ok())
                .and_then(|secs| Timestamp::from_unix_timestamp(secs).ok())
            {
                return Some(ts);
            }
        }
    }

    // Regular message
    if let Some(duration) = QUEST_RE
        .captures(&message.content)
        .and_then(|captures| parse_duration_captures(captures))
    {
        return Some(Timestamp::from(message.timestamp.add(duration)));
    }

    // Terminal `to-do` command
    if let Some(ts) = QUEST_TODO_RE
        .captures(&message.content)
        .and_then(|captures| captures[5].parse().ok())
        .and_then(|secs| Timestamp::from_unix_timestamp(secs).ok())
    {
        return Some(ts);
    }

    None
}

pub fn extract_card_cooldown(message: &Message) -> Option<Timestamp> {
    if let Some(embed) = message.embeds.first() {
        // info command
        if let Some(description) = &embed.description {
            if let Some(ts) = CARD_TODO_RE
                .captures(description)
                .and_then(|captures| captures[5].parse().ok())
                .and_then(|secs| Timestamp::from_unix_timestamp(secs).ok())
            {
                return Some(ts);
            }
        }
    }

    // Terminal `to-do` command
    if let Some(ts) = CARD_TODO_RE
        .captures(&message.content)
        .and_then(|captures| captures[5].parse().ok())
        .and_then(|secs| Timestamp::from_unix_timestamp(secs).ok())
    {
        return Some(ts);
    }

    None
}

pub fn extract_profile_cooldown(message: &Message) -> Option<Timestamp> {
    if let Some(embed) = message.embeds.first() {
        // profile command
        // TODO can't construct an EmbedFooter to test
        if let Some(footer) = &embed.footer {
            if let Some(ts) = PROFILE_RE
                .captures(&footer.text)
                .and_then(|captures| parse_duration_captures(captures))
            {
                return Some(Timestamp::from(message.timestamp.add(ts)));
            }
        }
    }

    None
}

pub fn parse_duration(s: &str) -> Option<Duration> {
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(DURATION_PATTERN).unwrap());
    parse_duration_captures(RE.captures(s)?)
}

pub fn parse_duration_captures(captures: regex::Captures) -> Option<Duration> {
    let days: u64 = captures
        .get(1)
        .and_then(|s| s.as_str().parse().ok())
        .unwrap_or(0);
    let hours: u64 = captures
        .get(2)
        .and_then(|s| s.as_str().parse().ok())
        .unwrap_or(0);
    let minutes: u64 = captures[3].parse().ok()?;
    let seconds: u64 = captures[4].parse().ok()?;
    Some(Duration::from_secs(
        days * 86400 + hours * 3600 + minutes * 60 + seconds,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ZOO_USER_ID;
    use poise::serenity_prelude::{Embed, EmbedField};

    #[test]
    fn test_parse_duration() {
        assert_eq!(
            parse_duration("1d + 2:03:04"),
            Some(Duration::from_secs(86400 + 2 * 3600 + 3 * 60 + 4))
        );
        assert_eq!(
            parse_duration("10:25:53"),
            Some(Duration::from_secs(10 * 3600 + 25 * 60 + 53))
        );
        assert_eq!(
            parse_duration("36:58"),
            Some(Duration::from_secs(36 * 60 + 58))
        );
    }

    #[test]
    fn test_extract_rescue_cooldown_polar_star() {
        let mut message = Message::default();
        message.author.id = ZOO_USER_ID;
        let mut embed = Embed::default();
        embed
            .fields
            .push(EmbedField::new("🕓 Cooldown", "1d + 2:03:04", false));
        message.embeds.push(embed);
        assert_eq!(
            extract_rescue_cooldown(&message),
            Some(Timestamp::from_unix_timestamp(86400 + 2 * 3600 + 3 * 60 + 4).unwrap())
        );
    }

    #[test]
    fn test_extract_rescue_cooldown_regular() {
        let mut message = Message::default();
        message.author.id = ZOO_USER_ID;
        message.content = r"🪆 **User**, you can rescue another animal in **54:14**. Your quest will finish in **2.5 days**.".to_string();
        assert_eq!(
            extract_rescue_cooldown(&message),
            Some(Timestamp::from_unix_timestamp(54 * 60 + 14).unwrap())
        );
    }

    #[test]
    fn test_extract_rescue_cooldown_todo() {
        let mut message = Message::default();
        message.author.id = ZOO_USER_ID;
        message.content = r"`$ todo`
__**Upcoming Events**__
> 🐾 Next Rescue: **3:05:31** (<t:1711411463>)
> 🎴 Next Card Pull: **8:35:38** (<t:1711431269>)
> 🏕️ Quest Finishes: **2d + 06:41:20** (<t:1711597212>)"
            .to_string();
        assert_eq!(
            extract_rescue_cooldown(&message),
            Some(Timestamp::from_unix_timestamp(1711411463).unwrap())
        );
    }

    #[test]
    fn test_extract_rescue_cooldown_modifier() {
        let mut message = Message::default();
        message.author.id = ZOO_USER_ID;
        message.content = r"`$ z`
🐂🐂 You brought home a pair of **Oxen**! Lucky you!
<:energy_drink:979087891240210492> Cooldown raised by **44 minutes**! (finishes in 6:44:57)"
            .to_string();
        assert_eq!(
            extract_rescue_cooldown(&message),
            Some(Timestamp::from_unix_timestamp(6 * 3600 + 44 * 60 + 57).unwrap())
        );
    }

    #[test]
    fn test_extract_quest_cooldown_polar_star() {
        let mut message = Message::default();
        message.author.id = ZOO_USER_ID;
        let mut embed = Embed::default();
        embed
            .fields
            .push(EmbedField::new("🌲 Quest ends", "1d + 2:03:04", false));
        message.embeds.push(embed);
        assert_eq!(
            extract_quest_cooldown(&message),
            Some(Timestamp::from_unix_timestamp(86400 + 2 * 3600 + 3 * 60 + 4).unwrap())
        );
    }

    #[test]
    fn test_extract_quest_cooldown_regular() {
        let mut message = Message::default();
        message.author.id = ZOO_USER_ID;
        message.content = r"**User**, you can rescue another animal in **4:43:02**. Your quest will finish in **3:16:57**.".to_string();
        assert_eq!(
            extract_quest_cooldown(&message),
            Some(Timestamp::from_unix_timestamp(3 * 3600 + 16 * 60 + 57).unwrap())
        );
    }

    #[test]
    fn test_extract_quest_cooldown_todo() {
        let mut message = Message::default();
        message.author.id = ZOO_USER_ID;
        message.content = r"`$ todo`
__**Upcoming Events**__
> 🐾 Next Rescue: **3:05:31** (<t:1711411463>)
> 🎴 Next Card Pull: **8:35:38** (<t:1711431269>)
> 🏕️ Quest Finishes: **2d + 06:41:20** (<t:1711597212>)"
            .to_string();
        assert_eq!(
            extract_quest_cooldown(&message),
            Some(Timestamp::from_unix_timestamp(1711597212).unwrap())
        );
    }

    #[test]
    fn test_extract_card_cooldown_todo() {
        let mut message = Message::default();
        message.author.id = ZOO_USER_ID;
        message.content = r"`$ todo`
__**Upcoming Events**__
> 🐾 Next Rescue: **3:05:31** (<t:1711411463>)
> 🎴 Next Card Pull: **8:35:38** (<t:1711431269>)
> 🏕️ Quest Finishes: **2d + 06:41:20** (<t:1711597212>)"
            .to_string();
        assert_eq!(
            extract_card_cooldown(&message),
            Some(Timestamp::from_unix_timestamp(1711431269).unwrap())
        );
    }

    #[test]
    fn test_extract_rescue_cooldown_info() {
        let mut message = Message::default();
        message.author.id = ZOO_USER_ID;
        let mut embed = Embed::default();
        embed.description = Some(
            "🐾 Next Rescue: **3:20:57** (<t:1711424648>)\n\
            🎴 Next Card Pull: **6:50:51** (<t:1711437242>)\n\
            🌲 Quest Finishes: **1d + 20:46:58** (<t:1711573810>)"
                .to_string(),
        );
        message.embeds.push(embed);
        assert_eq!(
            extract_rescue_cooldown(&message),
            Some(Timestamp::from_unix_timestamp(1711424648).unwrap())
        );
    }

    #[test]
    fn test_extract_quest_cooldown_info() {
        let mut message = Message::default();
        message.author.id = ZOO_USER_ID;
        let mut embed = Embed::default();
        embed.description = Some(
            "🐾 Next Rescue: **3:20:57** (<t:1711424648>)\n\
            🎴 Next Card Pull: **6:50:51** (<t:1711437242>)\n\
            🌲 Quest Finishes: **1d + 20:46:58** (<t:1711573810>)"
                .to_string(),
        );
        message.embeds.push(embed);
        assert_eq!(
            extract_quest_cooldown(&message),
            Some(Timestamp::from_unix_timestamp(1711573810).unwrap())
        );
    }

    #[test]
    fn test_extract_card_cooldown_info() {
        let mut message = Message::default();
        message.author.id = ZOO_USER_ID;
        let mut embed = Embed::default();
        embed.description = Some(
            "🐾 Next Rescue: **3:20:57** (<t:1711424648>)\n\
            🎴 Next Card Pull: **6:50:51** (<t:1711437242>)\n\
            🌲 Quest Finishes: **1d + 20:46:58** (<t:1711573810>)"
                .to_string(),
        );
        message.embeds.push(embed);
        assert_eq!(
            extract_card_cooldown(&message),
            Some(Timestamp::from_unix_timestamp(1711437242).unwrap())
        );
    }
}
