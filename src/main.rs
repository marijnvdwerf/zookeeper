use std::{
    collections::{BTreeSet, HashSet},
    fmt::Display,
    ops::Sub,
    sync::Arc,
    time::Duration,
};

use anyhow::{Context as _, Error, Result};
use chrono::TimeDelta;
use poise::{builtins::register_globally, command, CreateReply, Framework, FrameworkOptions};
use serenity::{
    all::CreateEmbedFooter,
    builder::{
        CreateActionRow, CreateButton, CreateEmbed, CreateEmbedAuthor, CreateInteractionResponse,
        CreateInteractionResponseMessage,
    },
    cache::Cache,
    client::{ClientBuilder, Context as SerenityContext, FullEvent},
    gateway::ActivityData,
    http::{CacheHttp, Http},
    model::prelude::*,
    utils::{FormattedTimestamp, FormattedTimestampStyle},
    Client,
};
use tokio::{select, sync::RwLock, task, time};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::{error, info, warn};

mod parsers;
mod zoo;

use parsers::{
    extract_card_cooldown, extract_profile_cooldown, extract_quest_cooldown,
    extract_rescue_cooldown,
};
use zoo::{fetch_zoo_profile, profile_url};

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

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
enum CooldownKind {
    #[default]
    Rescue,
    Quest,
    Card,
    Profile,
}

impl CooldownKind {
    fn emoji(&self) -> &str {
        match self {
            CooldownKind::Rescue => "🐾",
            CooldownKind::Quest => "🏕️",
            CooldownKind::Card => "🎴",
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
    if let Some(timestamp) = extract_profile_cooldown(message) {
        cooldown_kinds.push((CooldownKind::Profile, timestamp));
    }
    if cooldown_kinds.is_empty() {
        return Ok(vec![]);
    }
    let profile = fetch_zoo_profile(&data.client, user_id.get(), None)
        .await
        .with_context(|| format!("Failed to fetch profile for user ID {}", user_id.get()))?;
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
    let config = data.config.read().await;
    if config.disabled_users.contains(&user_id) {
        return Ok(());
    }
    let manual = config.manual_users.contains(&user_id);
    drop(config);
    let cooldowns = extract_message_cooldowns(message, user_id, data).await?;
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
    for cooldown in extract_message_cooldowns(&message, user_id, data).await? {
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
                let message =
                    CreateInteractionResponseMessage::new().components(components).content(message);
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
    let mut description = String::new();
    for owner in &config.owners {
        if let Ok(user) = owner.to_user(ctx).await {
            description.push_str(&format!("**Created by:** {}\n", user.name));
        }
    }
    description.push_str(&format!("**Version:** v{}\n", env!("CARGO_PKG_VERSION")));
    description.push_str(&format!(
        "**Shard:** {}\n",
        data.shard.map_or("unknown".to_string(), |s| format!("{}/{}", s.id.0 + 1, s.total))
    ));
    description.push_str(&format!(
        "**Uptime:** {}\n",
        FormattedTimestamp::new(data.start_time, Some(FormattedTimestampStyle::RelativeTime))
    ));
    description.push_str(&format!("**Rust version:** v{}\n", env!("VERGEN_RUSTC_SEMVER")));
    description.push_str(&format!("**Memory usage:** {}\n", memory));
    description.push_str(&format!("**Tracked cooldowns:** {}\n", config.cooldowns.len()));
    let embed = CreateEmbed::default()
        .author(author)
        .description(description)
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
    ctx.send(CreateReply::default().content(message).components(components)).await?;
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
    let mut message = if cooldowns.is_empty() {
        if let Some(user) = &user {
            format!("No cooldowns tracked for {}.", user.mention())
        } else if show_all {
            format!("No cooldowns tracked in {}.", current_channel.mention())
        } else {
            "No cooldowns tracked. Use Zoo `/rescue` to start.".to_string()
        }
    } else {
        let mut output = if let Some(user) = &user {
            format!("Cooldowns tracked for {}:\n", user.mention())
        } else if show_all {
            format!("Cooldowns tracked in {}:\n", current_channel.mention())
        } else {
            "Your tracked cooldowns:\n".to_string()
        };
        for cooldown in cooldowns.iter().take(15) {
            if show_all {
                output.push_str(&format!(
                    "- {}: {}\n",
                    cooldown.user_id.mention(),
                    format_cooldown(cooldown)
                ));
            } else {
                output.push_str(&format!("- {}\n", format_cooldown(cooldown)));
            }
        }
        if cooldowns.len() > 15 {
            output.push_str(&format!("... and {} more", cooldowns.len() - 15));
        }
        output
    };

    if let Some(user) = &user {
        if config.manual_users.contains(&user.id) {
            message = format!("Auto mode: **disabled** ❌\n{}", message);
        } else {
            message = format!("Auto mode: **enabled** ✅\n{}", message);
        }
    } else if config.manual_users.contains(&current_user) {
        message = format!("Auto mode: **disabled** ❌\n{}", message);
    } else {
        message = format!("Auto mode: **enabled** ✅\n{}", message);
    }

    if let Some(user) = &user {
        if config.disabled_users.contains(&user.id) {
            message = format!("Tracking & notifications: **disabled** ❌\n{}", message);
        } else {
            message = format!("Tracking & notifications: **enabled** ✅\n{}", message);
        }
    } else if config.disabled_users.contains(&current_user) {
        message = format!("Tracking & notifications: **disabled** ❌\n{}", message);
    } else {
        message = format!("Tracking & notifications: **enabled** ✅\n{}", message);
    }

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
    (message, components)
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

fn format_cooldown(cooldown: &Cooldown) -> String {
    if cooldown.kind == CooldownKind::Profile {
        format!(
            "{} {} {}",
            cooldown.kind.emoji(),
            cooldown.kind,
            FormattedTimestamp::new(
                cooldown.timestamp,
                Some(FormattedTimestampStyle::RelativeTime)
            ),
        )
    } else {
        format!(
            "[**{}**](<{}>) {} {} {}",
            cooldown.profile_name,
            profile_url(cooldown.user_id.get(), Some(&cooldown.profile)),
            cooldown.kind.emoji(),
            cooldown.kind,
            FormattedTimestamp::new(
                cooldown.timestamp,
                Some(FormattedTimestampStyle::RelativeTime)
            ),
        )
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

async fn run_notifications(config: &RwLock<Config>, http: &MyCacheHttp) -> Result<(), Error> {
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
            let message = if cooldown.kind == CooldownKind::Profile {
                format!(
                    "{} {} {} cooldown finished",
                    cooldown.user_id.mention(),
                    cooldown.kind.emoji(),
                    cooldown.kind
                )
            } else {
                format!(
                    "{} {} {} cooldown finished for [**{}**](<{}>)\n```/profiles profile:{}```",
                    cooldown.user_id.mention(),
                    cooldown.kind.emoji(),
                    cooldown.kind,
                    cooldown.profile_name,
                    profile_url(cooldown.user_id.get(), Some(&cooldown.profile)),
                    cooldown.profile
                )
            };
            messages.push((cooldown.channel_id, message));
        }
    }
    if any_expired {
        config.cooldowns.retain(|cooldown| now < cooldown.timestamp);
        save_config(&config).await?;
    }
    drop(config);
    for (channel_id, message) in messages {
        if let Err(e) = channel_id.say(http, &message).await {
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

    let cloned_config = config.clone();
    let framework = Framework::builder()
        .options(FrameworkOptions {
            commands: vec![botstatus(), cooldowns(), disable(), enable()],
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
                    client: reqwest::Client::new(),
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
    tracker.spawn(task::spawn(async move {
        let mut interval = time::interval(Duration::from_millis(1000));
        loop {
            select! {
                _ = cloned_token.cancelled() => break,
                _ = interval.tick() => {},
            }
            match run_notifications(&cloned_config, &cache_http).await {
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
