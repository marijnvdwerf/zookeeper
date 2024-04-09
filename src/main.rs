use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fmt::Display,
    ops::Sub,
    sync::Arc,
    time::Duration,
};

use anyhow::{Context as _, Error, Result};
use chrono::TimeDelta;
use poise::{
    builtins::register_globally, command, CreateReply, Framework, FrameworkError, FrameworkOptions,
};
use serenity::{
    all::CreateMessage,
    builder::{
        CreateActionRow, CreateAllowedMentions, CreateButton, CreateEmbed, CreateEmbedAuthor,
        CreateEmbedFooter, CreateInteractionResponse, CreateInteractionResponseMessage,
    },
    cache::Cache,
    client::{ClientBuilder, Context as SerenityContext, FullEvent},
    gateway::ActivityData,
    http::{CacheHttp, Http},
    model::prelude::*,
    utils::{EmbedMessageBuilding, FormattedTimestamp, FormattedTimestampStyle, MessageBuilder},
    Client,
};
use tokio::{select, sync::RwLock, task, time};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::{error, info, warn};
use uuid::Uuid;

mod parsers;
mod zoo;

use parsers::{
    extract_card_cooldown, extract_mechanic_cooldown, extract_profile_cooldown,
    extract_quest_cooldown, extract_rescue_cooldown,
};
use zoo::{fetch_zoo_profile, profile_url, ZooProfileAnimal, ZooProfileResponse, ZooProfileResult};

struct Data {
    start_time: Timestamp,
    config: Arc<RwLock<Config>>,
    client: reqwest::Client,
    current_user: CurrentUser,
    shard: Option<ShardInfo>,
}
type Context<'a> = poise::Context<'a, Data, Error>;
type FrameworkContext<'a> = poise::FrameworkContext<'a, Data, Error>;

const ZOO_USER_ID: UserId = UserId::new(1008563327380766812);

const ANIMAL_NAMES: [&str; 100] = [
    "bat",
    "bear",
    "beaver",
    "beetle",
    "camel",
    "cat",
    "caterpillar",
    "chick",
    "chicken",
    "cow",
    "crab",
    "cricket",
    "crocodile",
    "dinosaur",
    "dog",
    "dove",
    "duck",
    "elephant",
    "fish",
    "fly",
    "fox",
    "frog",
    "giraffe",
    "gorilla",
    "hamster",
    "hedgehog",
    "hippo",
    "horse",
    "koala",
    "leopard",
    "lizard",
    "mouse",
    "ox",
    "parrot",
    "penguin",
    "pig",
    "rabbit",
    "seal",
    "sheep",
    "shrimp",
    "skunk",
    "sloth",
    "snail",
    "snowman",
    "spider",
    "squid",
    "turkey",
    "whale",
    "worm",
    "zebra",
    // rare animals
    "bactrian camel",
    "badger",
    "bee",
    "bird",
    "bison",
    "boar",
    "bunny",
    "butterfly",
    "chipmunk",
    "cockroach",
    "deer",
    "dodo",
    "dolphin",
    "dragon",
    "eagle",
    "flamingo",
    "goat",
    "kangaroo",
    "ladybug",
    "lion",
    "llama",
    "lobster",
    "mammoth",
    "monkey",
    "mosquito",
    "octopus",
    "orangutan",
    "otter",
    "owl",
    "panda",
    "peacock",
    "polar bear",
    "poodle",
    "pufferfish",
    "raccoon",
    "ram",
    "rat",
    "rhino",
    "rooster",
    "scorpion",
    "shark",
    "snake",
    "snowier man",
    "swan",
    "t-rex",
    "tiger",
    "tropical fish",
    "turtle",
    "unicorn",
    "wolf",
];

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
enum CooldownKind {
    #[default]
    Rescue,
    Quest,
    Card,
    Mechanic,
    Profile,
}

impl CooldownKind {
    fn emoji(&self) -> &str {
        match self {
            CooldownKind::Rescue => "🐾",
            CooldownKind::Quest => "🏕️",
            CooldownKind::Card => "🎴",
            CooldownKind::Mechanic => "🔧",
            CooldownKind::Profile => "👤",
        }
    }
}

impl Display for CooldownKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CooldownKind::Rescue => write!(f, "Rescue"),
            CooldownKind::Quest => write!(f, "Quest"),
            CooldownKind::Card => write!(f, "Card"),
            CooldownKind::Mechanic => write!(f, "Mechanic"),
            CooldownKind::Profile => write!(f, "Profile"),
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
struct Cooldown {
    #[serde(default)]
    kind: CooldownKind,
    channel_id: ChannelId,
    user_id: UserId,
    profile: String,
    profile_name: String,
    timestamp: Timestamp,
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
#[serde(default)]
struct Config {
    owners: Vec<UserId>,
    token: String,
    cooldowns: Vec<Cooldown>,
    disabled_users: BTreeSet<UserId>,
    manual_users: BTreeSet<UserId>,
    channel_users: BTreeMap<ChannelId, BTreeSet<UserId>>,
}

async fn load_config() -> Result<Config> {
    let config_path = std::env::var("CONFIG_PATH").unwrap_or_else(|_| "config.toml".to_string());
    if tokio::fs::metadata(&config_path).await.is_err() {
        return Ok(Config::default());
    }
    let config_str = tokio::fs::read_to_string(&config_path)
        .await
        .with_context(|| format!("Failed to read config file {}", config_path))?;
    toml::from_str(&config_str).context("Failed to deserialize config")
}

async fn save_config(config: &Config) -> Result<()> {
    let config_path = std::env::var("CONFIG_PATH").unwrap_or_else(|_| "config.toml".to_string());
    let string = toml::to_string(config).context("Failed to serialize config")?;
    tokio::fs::write(&config_path, string)
        .await
        .with_context(|| format!("Failed to write to {}", config_path))
}

async fn advertise_cooldowns(
    ctx: &SerenityContext,
    message: &Message,
    cooldowns: &[Cooldown],
    data: &Data,
) -> Result<()> {
    let mut updated = Vec::with_capacity(cooldowns.len());
    let config = data.config.read().await;
    for cooldown in cooldowns {
        if let Some(existing) = config.cooldowns.iter().find(|existing| {
            existing.kind == cooldown.kind
                && existing.user_id == cooldown.user_id
                && existing.profile == cooldown.profile
        }) {
            let diff =
                (existing.timestamp.unix_timestamp() - cooldown.timestamp.unix_timestamp()).abs();
            if diff > 2 {
                updated.push(existing.clone());
            }
        } else {
            updated.push(cooldown.clone());
        }
    }
    drop(config);
    for cooldown in &updated {
        info!(
            "Cooldown found: {} {} (user {}, profile {})",
            cooldown.kind, cooldown.timestamp, cooldown.user_id, cooldown.profile
        );
        let reaction = ReactionType::Unicode(cooldown.kind.emoji().to_string());
        message.react(ctx, reaction).await?;
    }
    Ok(())
}

async fn add_cooldowns(
    ctx: &SerenityContext,
    message: &Message,
    cooldowns: &[Cooldown],
    data: &Data,
) -> Result<()> {
    let mut updated = Vec::with_capacity(cooldowns.len());
    let mut config = data.config.write().await;
    for cooldown in cooldowns {
        if let Some(existing) = config.cooldowns.iter_mut().find(|existing| {
            existing.kind == cooldown.kind
                && existing.user_id == cooldown.user_id
                && existing.profile == cooldown.profile
        }) {
            // Update existing cooldown
            existing.channel_id = cooldown.channel_id;
            existing.profile_name = cooldown.profile_name.clone();
            // Check if timestamp is within 1 second of the existing one,
            // if not, update it
            let diff =
                (existing.timestamp.unix_timestamp() - cooldown.timestamp.unix_timestamp()).abs();
            if diff > 2 {
                existing.timestamp = cooldown.timestamp;
                updated.push(existing.clone());
            }
        } else {
            config.cooldowns.push(cooldown.clone());
            updated.push(cooldown.clone());
        }
    }
    save_config(&config).await?;
    drop(config);
    for cooldown in &updated {
        info!(
            "Cooldown added: {} {} (user {}, profile {})",
            cooldown.kind, cooldown.timestamp, cooldown.user_id, cooldown.profile
        );
    }
    if !updated.is_empty() {
        let reaction = ReactionType::Unicode("✅".to_string());
        message.react(ctx, reaction).await?;
    }
    Ok(())
}

async fn remove_cooldowns(
    ctx: &SerenityContext,
    message: &Message,
    cooldowns: &[Cooldown],
    data: &Data,
) -> Result<()> {
    let mut config = data.config.write().await;
    config.cooldowns.retain(|existing| {
        !cooldowns.iter().any(|cooldown| {
            existing.kind == cooldown.kind
                && existing.user_id == cooldown.user_id
                && existing.profile == cooldown.profile
        })
    });
    save_config(&config).await?;
    drop(config);
    for cooldown in cooldowns {
        info!(
            "Cooldown removed: {} {} (user {}, profile {})",
            cooldown.kind, cooldown.timestamp, cooldown.user_id, cooldown.profile
        );
        let reaction = ReactionType::Unicode("✅".to_string());
        message.delete_reaction(ctx, None, reaction).await?;
    }
    Ok(())
}

async fn extract_message_cooldowns(
    http: impl CacheHttp,
    message: &Message,
    user_id: UserId,
    data: &Data,
) -> Result<Vec<Cooldown>> {
    let mut cooldown_kinds = vec![];
    if let Some(timestamp) = extract_rescue_cooldown(message) {
        cooldown_kinds.push((CooldownKind::Rescue, timestamp));
    }
    if let Some(timestamp) = extract_card_cooldown(message) {
        cooldown_kinds.push((CooldownKind::Card, timestamp));
    }
    if let Some(timestamp) = extract_quest_cooldown(message) {
        cooldown_kinds.push((CooldownKind::Quest, timestamp));
    }
    if let Some(timestamp) = extract_mechanic_cooldown(message) {
        cooldown_kinds.push((CooldownKind::Mechanic, timestamp));
    }
    if let Some(timestamp) = extract_profile_cooldown(message) {
        cooldown_kinds.push((CooldownKind::Profile, timestamp));
    }
    if cooldown_kinds.is_empty() {
        return Ok(vec![]);
    }
    let Some(profile) = try_fetch_profile(&data.client, user_id, None).await else {
        message.react(http, ReactionType::Unicode("⚠️".to_string())).await?;
        return Ok(vec![]);
    };
    let cooldowns = cooldown_kinds
        .into_iter()
        .map(|(kind, timestamp)| Cooldown {
            kind,
            channel_id: message.channel_id,
            user_id,
            profile: profile.profile_id.clone(),
            profile_name: profile.name.clone(),
            timestamp,
        })
        .collect::<Vec<_>>();
    Ok(cooldowns)
}

async fn check_cooldown_message<'a>(
    ctx: &'a SerenityContext,
    message: &Message,
    data: &'a Data,
) -> Result<()> {
    if message.author.id != ZOO_USER_ID {
        return Ok(());
    }
    let Some(interaction) = message.interaction.as_deref() else {
        return Ok(());
    };
    let user_id = interaction.user.id;
    let mut config = data.config.write().await;
    // Add user to channel users if not already present
    if config.channel_users.entry(message.channel_id).or_insert_with(BTreeSet::new).insert(user_id)
    {
        save_config(&config).await?;
    }
    if config.disabled_users.contains(&user_id) {
        return Ok(());
    }
    let manual = config.manual_users.contains(&user_id);
    drop(config);
    let cooldowns = extract_message_cooldowns(ctx, message, user_id, data).await?;
    if cooldowns.is_empty() {
        return Ok(());
    }
    if manual {
        advertise_cooldowns(ctx, message, &cooldowns, data).await
    } else {
        add_cooldowns(ctx, message, &cooldowns, data).await
    }
}

async fn handle_reaction<'a>(
    ctx: &'a SerenityContext,
    add_reaction: &Reaction,
    data: &'a Data,
    add: bool,
) -> Result<()> {
    if add_reaction.member.as_ref().is_some_and(|m| m.user.bot) {
        return Ok(());
    }
    let Some(user_id) = add_reaction.user_id else {
        return Ok(());
    };
    let ReactionType::Unicode(emoji) = &add_reaction.emoji else {
        return Ok(());
    };
    let message = add_reaction.message(ctx).await.context("Fetching message for reaction")?;
    if message.author.id != ZOO_USER_ID {
        return Ok(());
    }
    let Some(interaction) = message.interaction.as_deref() else {
        return Ok(());
    };
    if user_id != interaction.user.id {
        return Ok(());
    }
    for cooldown in extract_message_cooldowns(ctx, &message, user_id, data).await? {
        if cooldown.kind.emoji() == emoji {
            if add {
                add_cooldowns(ctx, &message, &[cooldown], data).await?;
            } else {
                remove_cooldowns(ctx, &message, &[cooldown], data).await?;
            }
            break;
        }
    }
    Ok(())
}

async fn handle_interaction<'a>(
    ctx: &'a SerenityContext,
    interaction: &'a Interaction,
    data: &'a Data,
) -> Result<()> {
    if let Interaction::Component(component) = interaction {
        let Some(interaction) = component.message.interaction.as_deref() else {
            return Ok(());
        };
        if interaction.user.id != component.user.id {
            component
                .create_response(
                    ctx,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("You can't do that!")
                            .ephemeral(true),
                    ),
                )
                .await?;
            return Ok(());
        }
        match component.data.custom_id.as_str() {
            "disable" | "enable" | "auto" | "manual" | "all" => {
                let mut config = data.config.write().await;
                if component.data.custom_id == "enable" {
                    config.disabled_users.remove(&component.user.id);
                } else if component.data.custom_id == "disable" {
                    config.disabled_users.insert(component.user.id);
                } else if component.data.custom_id == "auto" {
                    config.manual_users.remove(&component.user.id);
                } else if component.data.custom_id == "manual" {
                    config.manual_users.insert(component.user.id);
                }
                save_config(&config).await?;
                let (message, components) = create_cooldowns_message(
                    &config,
                    None,
                    component.data.custom_id == "all",
                    component.user.id,
                    component.channel_id,
                );
                drop(config);
                let message = CreateInteractionResponseMessage::new()
                    .components(components)
                    .content(message)
                    .allowed_mentions(CreateAllowedMentions::new());
                component
                    .create_response(ctx, CreateInteractionResponse::UpdateMessage(message))
                    .await?;
            }
            _ => {
                warn!("Unknown interaction component ID: {}", component.data.custom_id);
                component.create_response(ctx, CreateInteractionResponse::Acknowledge).await?;
            }
        }
    }
    Ok(())
}

async fn event_handler<'a>(
    ctx: &'a SerenityContext,
    event: &'a FullEvent,
    _framework: FrameworkContext<'a>,
    data: &'a Data,
) -> Result<()> {
    // debug!("Event: {:?}", event);
    match event {
        FullEvent::Message { new_message: message } => {
            if let Err(e) = check_cooldown_message(ctx, message, data).await {
                error!("Error handling message: {:?}", e);
            }
        }
        FullEvent::ReactionAdd { add_reaction } => {
            if let Err(e) = handle_reaction(ctx, add_reaction, data, true).await {
                error!("Error handling reaction: {:?}", e);
            }
        }
        FullEvent::ReactionRemove { removed_reaction } => {
            if let Err(e) = handle_reaction(ctx, removed_reaction, data, false).await {
                error!("Error handling reaction: {:?}", e);
            }
        }
        FullEvent::InteractionCreate { interaction } => {
            if let Err(e) = handle_interaction(ctx, interaction, data).await {
                error!("Error handling interaction: {:?}", e);
            }
        }
        _ => {}
    }
    Ok(())
}

/// View some details about the bot
#[command(slash_command, ephemeral)]
async fn botstatus(ctx: Context<'_>) -> Result<(), Error> {
    let ping = ctx.ping().await;
    let data = ctx.data();
    let config = data.config.read().await;
    let memory = memory_stats::memory_stats()
        .map(|s| human_bytes::human_bytes(s.physical_mem as f64))
        .unwrap_or_else(|| "<unknown>".to_string());
    let mut author = CreateEmbedAuthor::new(data.current_user.name.clone());
    if let Some(avatar_url) = data.current_user.avatar_url() {
        author = author.icon_url(avatar_url);
    }
    let mut description = MessageBuilder::new();
    for owner in &config.owners {
        if let Ok(user) = owner.to_user(ctx).await {
            description.push_bold("Created by: ").push_line_safe(user.name);
        }
    }
    description.push_bold("Version: ").push_line(env!("CARGO_PKG_VERSION"));
    description.push_bold("Shard: ").push_line(
        data.shard.map_or("unknown".to_string(), |s| format!("{}/{}", s.id.0 + 1, s.total)),
    );
    description.push_bold("Uptime: ").push_line(
        FormattedTimestamp::new(data.start_time, Some(FormattedTimestampStyle::RelativeTime))
            .to_string(),
    );
    description.push_bold("Rust version: ").push(env!("VERGEN_RUSTC_SEMVER")).push_line(" 🦀");
    description.push_bold("Memory usage: ").push_line(memory);
    description.push_bold("Tracked cooldowns: ").push_line(config.cooldowns.len().to_string());
    let embed = CreateEmbed::default()
        .author(author)
        .description(description.build())
        .footer(CreateEmbedFooter::new(format!("Ping: {}ms", ping.as_millis())));
    drop(config);
    ctx.send(CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// List all tracked cooldowns
#[command(slash_command, ephemeral)]
async fn cooldowns(
    ctx: Context<'_>,
    #[description = "Selected user"] user: Option<User>,
) -> Result<(), Error> {
    let config = ctx.data().config.read().await;
    let (message, components) =
        create_cooldowns_message(&config, user, false, ctx.author().id, ctx.channel_id());
    drop(config);
    let reply = CreateReply::default()
        .content(message)
        .components(components)
        .allowed_mentions(CreateAllowedMentions::new());
    ctx.send(reply).await?;
    Ok(())
}

fn create_cooldowns_message(
    config: &Config,
    user: Option<User>,
    show_all: bool,
    current_user: UserId,
    current_channel: ChannelId,
) -> (String, Vec<CreateActionRow>) {
    let mut cooldowns = config
        .cooldowns
        .iter()
        .filter(|cooldown| {
            if let Some(user) = &user {
                cooldown.user_id == user.id
            } else {
                cooldown.user_id == current_user
                    || (show_all && cooldown.channel_id == current_channel)
            }
        })
        .collect::<Vec<_>>();
    cooldowns.sort_by_key(|cooldown| cooldown.timestamp);

    let mut message = MessageBuilder::new();
    message.push("Tracking & notifications: ");
    if let Some(user) = &user {
        if config.disabled_users.contains(&user.id) {
            message.push_bold("disabled").push_line(" ❌");
        } else {
            message.push_bold("enabled").push_line(" ✅");
        }
    } else if config.disabled_users.contains(&current_user) {
        message.push_bold("disabled").push_line(" ❌");
    } else {
        message.push_bold("enabled").push_line(" ✅");
    }

    message.push("Auto mode: ");
    if let Some(user) = &user {
        if config.manual_users.contains(&user.id) {
            message.push_bold("disabled").push_line(" ❌");
        } else {
            message.push_bold("enabled").push_line(" ✅");
        }
    } else if config.manual_users.contains(&current_user) {
        message.push_bold("disabled").push_line(" ❌");
    } else {
        message.push_bold("enabled").push_line(" ✅");
    }

    if cooldowns.is_empty() {
        if let Some(user) = &user {
            message.push("No cooldowns tracked for ").user(user).push_line(".");
        } else if show_all {
            message.push("No cooldowns tracked in ").channel(current_channel).push_line(".");
        } else {
            message.push_line("No cooldowns tracked. Use Zoo `/rescue` to start.");
        }
    } else {
        if let Some(user) = &user {
            message.push("Cooldowns tracked for ").user(user).push_line(":");
        } else if show_all {
            message.push("Cooldowns tracked in ").channel(current_channel).push_line(":");
        } else {
            message.push_line("Your tracked cooldowns:");
        };
        for cooldown in cooldowns.iter().take(15) {
            if show_all {
                message
                    .push("- ")
                    .user(cooldown.user_id)
                    .push(": ")
                    .push_line(format_cooldown(cooldown));
            } else {
                message.push("- ").push_line(format_cooldown(cooldown));
            }
        }
        if cooldowns.len() > 15 {
            message.push_line(format!("... and {} more", cooldowns.len() - 15));
        }
    };

    let mut components = vec![];
    if user.is_none() {
        let mut buttons = vec![];
        if config.disabled_users.contains(&current_user) {
            buttons.push(CreateButton::new("enable").label("Enable").style(ButtonStyle::Success));
        } else {
            buttons.push(CreateButton::new("disable").label("Disable").style(ButtonStyle::Danger));
        }
        if config.manual_users.contains(&current_user) {
            buttons.push(CreateButton::new("auto").label("Auto mode").style(ButtonStyle::Primary));
        } else {
            buttons.push(
                CreateButton::new("manual").label("Manual mode").style(ButtonStyle::Secondary),
            );
        }
        if !show_all && config.owners.contains(&current_user) {
            buttons.push(CreateButton::new("all").label("Show all").style(ButtonStyle::Secondary));
        }
        components.push(CreateActionRow::Buttons(buttons));
    }
    (message.build(), components)
}

/// Disable bot tracking and notifications
#[command(slash_command, ephemeral)]
async fn disable(ctx: Context<'_>) -> Result<(), Error> {
    let mut config = ctx.data().config.write().await;
    config.disabled_users.insert(ctx.author().id);
    save_config(&config).await?;
    drop(config);
    ctx.say("No longer tracking your cooldowns or sending notifications.\nUse `/enable` to start again.")
        .await?;
    Ok(())
}

/// Enable bot tracking and notifications
#[command(slash_command, ephemeral)]
async fn enable(ctx: Context<'_>) -> Result<(), Error> {
    let mut config = ctx.data().config.write().await;
    config.disabled_users.remove(&ctx.author().id);
    save_config(&config).await?;
    drop(config);
    ctx.say("Tracking your cooldowns and sending notifications.\nUse `/disable` to stop.").await?;
    Ok(())
}

async fn try_fetch_profile(
    client: &reqwest::Client,
    user_id: UserId,
    profile: Option<&str>,
) -> Option<Box<ZooProfileResponse>> {
    match fetch_zoo_profile(client, user_id.get(), profile).await {
        Ok(ZooProfileResult::Profile(profile)) => Some(profile),
        Ok(ZooProfileResult::Invalid(error)) => {
            warn!("Failed to fetch profile for {}: {:?}", user_id, error);
            None
        }
        Ok(ZooProfileResult::ApiError(error)) => {
            warn!("Failed to fetch profile for {}: {:?}", user_id, error);
            None
        }
        Err(e) => {
            warn!("Failed to fetch profile {}: {:?}", user_id, e);
            None
        }
    }
}

/// Find an animal in any channel user's profile
#[command(slash_command)]
async fn find(ctx: Context<'_>, #[description = "Animal name"] name: String) -> Result<(), Error> {
    // Start typing to show that the bot is searching
    ctx.defer().await?;

    if !ANIMAL_NAMES.iter().any(|animal| animal.eq_ignore_ascii_case(&name)) {
        let mut message = MessageBuilder::new();
        message.push_bold_safe(&name).push(" is not a valid animal.");

        let reply = CreateReply::default()
            .content(message.build())
            .allowed_mentions(CreateAllowedMentions::new());
        ctx.send(reply).await?;
        return Ok(());
    }

    let config = ctx.data().config.read().await;
    let user_ids = config
        .channel_users
        .get(&ctx.channel_id())
        .into_iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    let mut profiles = vec![];
    let mut failed_profiles = false;
    for user_id in user_ids {
        let Some(profile) = try_fetch_profile(&ctx.data().client, user_id, None).await else {
            failed_profiles = true;
            continue;
        };
        // Also fetch other profiles from the same user
        for profile_name in &profile.profiles {
            if profile_name == &profile.profile_id {
                continue;
            }
            let Some(profile) =
                try_fetch_profile(&ctx.data().client, user_id, Some(profile_name.as_str())).await
            else {
                failed_profiles = true;
                continue;
            };
            profiles.push(profile);
        }
        profiles.push(profile);
    }
    drop(config);
    struct FoundAnimal<'a> {
        profile: &'a ZooProfileResponse,
        animal: &'a ZooProfileAnimal,
        // Profile also has the rare version of the animal
        has_rare: bool,
    }
    let mut found = vec![];
    for profile in &profiles {
        if let Some(animal) = profile
            .animals
            .iter()
            .find(|animal| animal.amount > 0 && animal.name.eq_ignore_ascii_case(&name))
        {
            let has_rare = !animal.rare
                && profile
                    .animals
                    .iter()
                    .any(|v| v.rare && v.amount > 0 && v.family == animal.family);
            found.push(FoundAnimal { profile, animal, has_rare });
        }
    }
    found.sort_by(|a, b| {
        // Pinned animals last, then profiles with rare first, then by amount
        a.animal.pinned.cmp(&b.animal.pinned).then_with(|| {
            b.has_rare.cmp(&a.has_rare).then_with(|| b.animal.amount.cmp(&a.animal.amount))
        })
    });
    let mut message = MessageBuilder::new();
    if failed_profiles {
        message.push_line("⚠️ Some profiles couldn't be fetched, results may be incomplete.");
    }
    if found.is_empty() {
        message
            .push("Couldn't find ")
            .push_bold_safe(&name)
            .push(format!(" in {} profiles.", profiles.len()));
    } else {
        // let mut message = format!("Found **{}** in {} profiles:\n", name, found.len());
        message
            .push("Found ")
            .push_bold_safe(&name)
            .push_line(format!(" in {} profiles:", found.len()));
        for found in found.iter().take(10) {
            let user_id: UserId = found.profile.user_id.parse()?;
            message
                .push("- ")
                .push_bold(format!("{}x", found.animal.amount))
                .push(" in ")
                .push(profile_link(&found.profile.name, user_id, Some(&found.profile.profile_id)));
            if found.has_rare {
                message.push(" 🌟");
            }
            if found.animal.pinned {
                message.push(" 📌");
            }
            message.push_line("");
        }
        if found.len() > 10 {
            message.push_line(format!("... and {} more", found.len() - 10));
        }
    }
    let reply = CreateReply::default()
        .content(message.build())
        .allowed_mentions(CreateAllowedMentions::new());
    ctx.send(reply).await?;
    Ok(())
}

fn format_cooldown(cooldown: &Cooldown) -> String {
    let cooldown_msg = format!(
        "{} {} {}",
        cooldown.kind.emoji(),
        cooldown.kind,
        FormattedTimestamp::new(cooldown.timestamp, Some(FormattedTimestampStyle::RelativeTime)),
    );
    if cooldown.kind == CooldownKind::Profile {
        cooldown_msg
    } else {
        MessageBuilder::new()
            .push(profile_link(&cooldown.profile_name, cooldown.user_id, Some(&cooldown.profile)))
            .push(" ")
            .push(cooldown_msg)
            .build()
    }
}

struct MyCacheHttp {
    cache: Arc<Cache>,
    http: Arc<Http>,
}

impl MyCacheHttp {
    fn new(client: &Client) -> Self {
        Self { cache: client.cache.clone(), http: client.http.clone() }
    }
}

impl CacheHttp for MyCacheHttp {
    fn http(&self) -> &Http { &self.http }

    fn cache(&self) -> Option<&Arc<Cache>> { Some(&self.cache) }
}

async fn run_notifications(
    config: &RwLock<Config>,
    http: &MyCacheHttp,
    client: &reqwest::Client,
) -> Result<(), Error> {
    let mut config = config.write().await;
    let now = Timestamp::now();
    let mut any_expired = false;
    let mut messages = vec![];
    for cooldown in &config.cooldowns {
        if now >= cooldown.timestamp {
            info!(
                "{} cooldown finished: {} (user {}, profile {})",
                cooldown.kind, cooldown.timestamp, cooldown.user_id, cooldown.profile
            );
            any_expired = true;
            if config.disabled_users.contains(&cooldown.user_id)
                // Don't notify if it expired more than 10 minutes ago
                || *cooldown.timestamp < now.sub(TimeDelta::try_minutes(10).unwrap())
            {
                // Remove but don't notify
                continue;
            }
            let mut message = MessageBuilder::new();
            message
                .user(cooldown.user_id)
                .push(format!(" {} {}", cooldown.kind.emoji(), cooldown.kind))
                .push(" cooldown finished");
            if cooldown.kind != CooldownKind::Profile {
                message.push(" for ").push(profile_link(
                    &cooldown.profile_name,
                    cooldown.user_id,
                    Some(&cooldown.profile),
                ));
                let result = fetch_zoo_profile(client, cooldown.user_id.get(), None).await;
                if let Ok(ZooProfileResult::Profile(current_profile)) = result {
                    if current_profile.profile_id == cooldown.profile {
                        message.push(" (current profile)");
                    } else {
                        message
                            .push("\n\nCurrent profile: ")
                            .push(profile_link(
                                &current_profile.name,
                                cooldown.user_id,
                                Some(&current_profile.profile_id),
                            ))
                            .push(". Switch profiles with: ")
                            .push_codeblock_safe(
                                format!("/profiles profile:{}", cooldown.profile),
                                None,
                            );
                    }
                } else {
                    message.push(" (⚠️ failed to fetch current profile)");
                }
            }
            let reply = CreateMessage::default()
                .content(message.build())
                .allowed_mentions(CreateAllowedMentions::new().users([cooldown.user_id]));
            messages.push((cooldown.channel_id, reply));
        }
    }
    if any_expired {
        config.cooldowns.retain(|cooldown| now < cooldown.timestamp);
        save_config(&config).await?;
    }
    drop(config);
    for (channel_id, message) in messages {
        if let Err(e) = channel_id.send_message(http, message).await {
            error!("Failed to send message: {:?}", e);
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let mut config = load_config().await.unwrap();
    if config.token.is_empty() {
        config.token = std::env::var("DISCORD_TOKEN").expect("missing DISCORD_TOKEN");
    }
    let api_token = config.token.clone();
    let owners = HashSet::from_iter(config.owners.iter().cloned());
    let config = Arc::new(RwLock::new(config));
    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::GUILD_MESSAGE_REACTIONS
        | GatewayIntents::MESSAGE_CONTENT;
    let reqwest_client = reqwest::Client::new();

    let cloned_config = config.clone();
    let cloned_reqwest_client = reqwest_client.clone();
    let framework = Framework::builder()
        .options(FrameworkOptions {
            commands: vec![botstatus(), cooldowns(), disable(), enable(), find()],
            on_error: |error| {
                Box::pin(async move {
                    if let Err(e) = on_error(error).await {
                        error!("Error while handling error: {:?}", e);
                    }
                })
            },
            event_handler: |ctx, event, framework, data| {
                Box::pin(event_handler(ctx, event, framework, data))
            },
            pre_command: |ctx| {
                info!(
                    "User {} ({}) used: {}",
                    ctx.author().name,
                    ctx.author().id,
                    ctx.invocation_string()
                );
                Box::pin(async move {})
            },
            owners,
            ..Default::default()
        })
        .setup(|ctx, ready, framework| {
            Box::pin(async move {
                ctx.set_presence(
                    Some(ActivityData::custom("/cooldowns")),
                    OnlineStatus::DoNotDisturb,
                );
                register_globally(ctx, &framework.options().commands).await?;
                Ok(Data {
                    start_time: Timestamp::now(),
                    config: cloned_config,
                    client: cloned_reqwest_client,
                    current_user: ready.user.clone(),
                    shard: ready.shard,
                })
            })
        })
        .build();

    let mut client = ClientBuilder::new(api_token, intents).framework(framework).await.unwrap();

    let tracker = TaskTracker::new();
    let token = CancellationToken::new();
    let cloned_token = token.clone();
    let cloned_config = config.clone();
    let cache_http = MyCacheHttp::new(&client);
    let cloned_reqwest_client = reqwest_client.clone();
    tracker.spawn(task::spawn(async move {
        let mut interval = time::interval(Duration::from_millis(1000));
        loop {
            select! {
                _ = cloned_token.cancelled() => break,
                _ = interval.tick() => {},
            }
            match run_notifications(&cloned_config, &cache_http, &cloned_reqwest_client).await {
                Ok(()) => {}
                Err(e) => {
                    error!("Error running notifications: {:?}", e);
                }
            }
        }
    }));

    let shard_manager = client.shard_manager.clone();
    let cloned_token = token.clone();
    tokio::spawn(async move {
        select! {
            _ = cloned_token.cancelled() => {},
            _ = tokio::signal::ctrl_c() => {},
        }
        shard_manager.shutdown_all().await;
    });

    info!("Starting client...");
    if let Err(why) = client.start_autosharded().await {
        error!("Client error: {:?}", why);
    }
    info!("Shutting down gracefully...");
    token.cancel();
    tracker.close();
    tracker.wait().await;
    let guard = config.read().await;
    save_config(&guard).await.unwrap();
}

async fn on_error(error: FrameworkError<'_, Data, Error>) -> Result<()> {
    match error {
        FrameworkError::Setup { error, .. } => {
            error!("User data setup error: {:?}", error);
        }
        FrameworkError::EventHandler { error, event, .. } => {
            error!("Event {} handler error: {:?}", event.snake_case_name(), error)
        }
        FrameworkError::Command { ctx, error, .. } => {
            let error_id = Uuid::new_v4();
            error!("Command error {}: {:?}", error_id, error);
            let embed = CreateEmbed::new()
                .title("⚠️ Error")
                .description("Command failed.")
                .field("Message", error.to_string(), false)
                .field("Error ID", error_id.to_string(), false)
                .color(Colour::RED);
            let reply = CreateReply::default()
                .embed(embed)
                .ephemeral(true)
                .allowed_mentions(CreateAllowedMentions::new());
            ctx.send(reply).await?;
        }
        _ => poise::builtins::on_error(error).await?,
    }
    Ok(())
}

fn profile_link(name: &str, user_id: UserId, profile: Option<&str>) -> String {
    let mut message = MessageBuilder::new();
    let name = MessageBuilder::new().push_bold_safe(name).build();
    message.push_named_link_safe(name, format!("<{}>", profile_url(user_id.get(), profile)));
    message.build()
}
