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
    builder::{CreateEmbed, CreateEmbedAuthor},
    client::{ClientBuilder, Context as SerenityContext, FullEvent},
    http::Http,
    model::prelude::*,
    utils::{FormattedTimestamp, FormattedTimestampStyle},
};
use tokio::{select, sync::Mutex, task, time};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::{error, info};

mod parsers;
mod zoo;

use parsers::{
    extract_card_cooldown, extract_profile_cooldown, extract_quest_cooldown,
    extract_rescue_cooldown,
};
use zoo::{fetch_zoo_profile, profile_url};

struct Data {
    start_time: Timestamp,
    config: Arc<Mutex<Config>>,
    client: reqwest::Client,
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

async fn handle_cooldowns(
    ctx: &SerenityContext,
    message: &Message,
    cooldowns: &[Cooldown],
    data: &Data,
) -> Result<()> {
    {
        let mut config = data.config.lock().await;
        for cooldown in cooldowns {
            if let Some(existing) = config.cooldowns.iter_mut().find(|existing| {
                existing.kind == cooldown.kind
                    && existing.user_id == cooldown.user_id
                    && existing.profile == cooldown.profile
            }) {
                // Update existing cooldown
                existing.channel_id = cooldown.channel_id;
                existing.profile_name = cooldown.profile_name.clone();
                existing.timestamp = cooldown.timestamp;
            } else {
                config.cooldowns.push(cooldown.clone());
            }
        }
        save_config(&config).await?;
    }
    for cooldown in cooldowns {
        info!(
            "Cooldown captured: {} {} (user {}, profile {})",
            cooldown.kind, cooldown.timestamp, cooldown.user_id, cooldown.profile
        );
        let reaction = ReactionType::Unicode(cooldown.kind.emoji().to_string());
        message.react(ctx, reaction).await?;
    }
    Ok(())
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
    {
        let config = data.config.lock().await;
        if config.disabled_users.contains(&user_id) {
            return Ok(());
        }
    }
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
        return Ok(());
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
    handle_cooldowns(ctx, message, &cooldowns, data).await
}

async fn event_handler<'a>(
    ctx: &'a SerenityContext,
    event: &'a FullEvent,
    _framework: FrameworkContext<'a>,
    data: &'a Data,
) -> Result<()> {
    // debug!("Event: {:?}", event);
    if let FullEvent::Message { new_message: message } = event {
        if let Err(e) = check_cooldown_message(ctx, message, data).await {
            error!("Error handling message: {:?}", e);
        }
    }
    Ok(())
}

/// View some details about the bot
#[command(slash_command, ephemeral)]
async fn botstatus(ctx: Context<'_>) -> Result<(), Error> {
    let config = ctx.data().config.lock().await;
    let memory = memory_stats::memory_stats()
        .map(|s| human_bytes::human_bytes(s.physical_mem as f64))
        .unwrap_or_else(|| "<unknown>".to_string());
    let embed = CreateEmbed::default()
        .author(CreateEmbedAuthor::new("Zookeeper").icon_url("https://cdn.discordapp.com/avatars/1221853228115693608/40b9e887ade5ce25f5e14112c6f5e6fb"))
        .description(format!(
            "**Created by:** encounter\n\
            **Version:** v{}\n\
            **Uptime:** {}\n\
            **Rust version:** v{}\n\
            **Memory usage:** {}\n\
            **Tracked cooldowns:** {}",
            env!("CARGO_PKG_VERSION"),
            FormattedTimestamp::new(ctx.data().start_time, Some(FormattedTimestampStyle::RelativeTime)),
            env!("VERGEN_RUSTC_SEMVER"),
            memory,
            config.cooldowns.len()
        ));
    ctx.send(CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// List all tracked cooldowns
#[command(slash_command, ephemeral)]
async fn cooldowns(
    ctx: Context<'_>,
    #[description = "Selected user"] user: Option<User>,
) -> Result<(), Error> {
    let config = ctx.data().config.lock().await;
    let show_all = user.is_none() && ctx.framework().options.owners.contains(&ctx.author().id);
    let mut cooldowns = config
        .cooldowns
        .iter()
        .filter(|cooldown| {
            if let Some(user) = &user {
                cooldown.user_id == user.id
            } else {
                cooldown.user_id == ctx.author().id
                    || (show_all && cooldown.channel_id == ctx.channel_id())
            }
        })
        .collect::<Vec<_>>();
    cooldowns.sort_by_key(|cooldown| cooldown.timestamp);
    let mut message = if cooldowns.is_empty() {
        if let Some(user) = &user {
            format!("No cooldowns tracked for {}.", user.mention())
        } else if show_all {
            format!("No cooldowns tracked in {}.", ctx.channel_id().mention())
        } else {
            "No cooldowns tracked. Use Zoo `/rescue` to start.".to_string()
        }
    } else {
        let mut output = if let Some(user) = &user {
            format!("Cooldowns tracked for {}:\n", user.mention())
        } else if show_all {
            format!("Cooldowns tracked in {}:\n", ctx.channel_id().mention())
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
        if config.disabled_users.contains(&user.id) {
            message = format!("Tracking & notifications: **disabled** ❌\n{}", message);
        } else {
            message = format!("Tracking & notifications: **enabled** ✅\n{}", message);
        }
    } else if !show_all {
        if config.disabled_users.contains(&ctx.author().id) {
            message = format!("Tracking & notifications: **disabled** ❌\n{}", message);
        } else {
            message = format!("Tracking & notifications: **enabled** ✅\n{}", message);
        }
    }

    ctx.say(message).await?;
    Ok(())
}

/// Disable bot tracking and notifications
#[command(slash_command, ephemeral)]
async fn disable(ctx: Context<'_>) -> Result<(), Error> {
    {
        let mut config = ctx.data().config.lock().await;
        config.disabled_users.insert(ctx.author().id);
        save_config(&config).await?;
    }
    ctx.say("No longer tracking your cooldowns or sending notifications.\nUse `/enable` to start again.")
        .await?;
    Ok(())
}

/// Enable bot tracking and notifications
#[command(slash_command, ephemeral)]
async fn enable(ctx: Context<'_>) -> Result<(), Error> {
    {
        let mut config = ctx.data().config.lock().await;
        config.disabled_users.remove(&ctx.author().id);
        save_config(&config).await?;
    }
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

async fn run_notifications(config: &mut Config, http: &Http) -> Result<(), Error> {
    let now = Timestamp::now();
    let mut any_expired = false;
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
            if let Err(e) = cooldown.channel_id.say(http, &message).await {
                error!("Failed to send message: {:?}", e);
            }
        }
    }
    if any_expired {
        config.cooldowns.retain(|cooldown| now < cooldown.timestamp);
        save_config(config).await?;
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
    let config = Arc::new(Mutex::new(config));
    let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT;

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
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                register_globally(ctx, &framework.options().commands).await?;
                Ok(Data {
                    start_time: Timestamp::now(),
                    config: cloned_config,
                    client: reqwest::Client::new(),
                })
            })
        })
        .build();

    let mut client = ClientBuilder::new(api_token, intents).framework(framework).await.unwrap();

    let tracker = TaskTracker::new();
    let token = CancellationToken::new();
    let cloned_token = token.clone();
    let cloned_config = config.clone();
    let http = client.http.clone();
    tracker.spawn(task::spawn(async move {
        let mut interval = time::interval(Duration::from_millis(1000));
        loop {
            select! {
                _ = cloned_token.cancelled() => break,
                _ = interval.tick() => {},
            }
            let mut config = cloned_config.lock().await;
            match run_notifications(&mut config, &http).await {
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
    if let Err(why) = client.start().await {
        error!("Client error: {:?}", why);
    }
    info!("Shutting down gracefully...");
    token.cancel();
    tracker.close();
    tracker.wait().await;
    let guard = config.lock_owned().await;
    save_config(&guard).await.unwrap();
}
