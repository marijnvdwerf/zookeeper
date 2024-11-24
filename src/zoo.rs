use anyhow::{Error, Result};

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileUser {
    pub avatar: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileUniqueAnimals {
    pub common: u32,
    pub rare: u32,
    pub total: u32,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileTotalAnimals {
    pub common: u32,
    pub rare: u32,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileAnimal {
    pub name: String,
    pub amount: u32,
    pub emoji: String,
    #[serde(rename = "emojiName")]
    pub emoji_name: String,
    pub family: String,
    pub rare: bool,
    pub pinned: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileRelic {
    pub name: String,
    pub emoji: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileCosmetic {
    pub name: String,
    pub emoji: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileLeader {
    pub name: String,
    pub emoji: String,
    pub triggered: u32,
    pub xp: u32,
    pub level: u32,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileQuest {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub emoji: String,
    pub days: f32,
    #[serde(default)]
    pub mins: u32,
    pub completed: u32,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileActiveQuest {
    #[serde(rename = "type")]
    pub kind: String,
    pub animal: String,
    pub family: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileCurse {
    pub name: String,
    pub names: ZooProfileCurseNames,
    pub weak: bool,
    pub effects: ZooProfileCurseEffects,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileCurseNames {
    #[serde(rename = "type")]
    pub kind: String,
    pub cure: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileCurseEffects {
    #[serde(rename = "type")]
    pub kind: ZooProfileCurseEffect,
    pub cure: ZooProfileCurseEffect,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileCurseEffect {
    pub name: String,
    pub description: String,
    // TODO nullable?
    // pub weak: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileTerminalFishy {
    #[serde(rename = "commonFish")]
    pub common_fish: u32,
    #[serde(rename = "uncommonFish")]
    pub uncommon_fish: u32,
    #[serde(rename = "rareFish")]
    pub rare_fish: u32,
    pub trash: u32,
    pub pebbles: u32,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileTerminalGarden {
    pub unlocked: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileTerminalCards {
    pub total: u32,
    pub rarities: ZooProfileTerminalCardsRarities,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileTerminalCardsRarities {
    pub c: u32,
    pub r: u32,
    pub d: u32,
    pub l: u32,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileTerminalFusionFusions {
    #[serde(rename = "commonCommon")]
    pub common_common: u32,
    #[serde(rename = "commonRare")]
    pub common_rare: u32,
    #[serde(rename = "rareRare")]
    pub rare_rare: u32,
    pub total: u32,
    pub score: u32,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileTerminalFusionNfbs {
    pub common: u32,
    pub rare: u32,
    pub total: u32,
    pub score: u32,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileTerminalFusion {
    #[serde(rename = "tokensPerRescue")]
    pub tokens_per_rescue: u32,
    #[serde(rename = "tokensFromFusions")]
    pub tokens_from_fusions: u32,
    #[serde(rename = "nfbMultiplier")]
    pub nfb_multiplier: f32,
    pub fusions: ZooProfileTerminalFusionFusions,
    pub nfbs: ZooProfileTerminalFusionNfbs,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileTerminal {
    pub unlocked: bool,
    #[serde(default)]
    pub admin: bool,
    #[serde(default, rename = "commandsFound")]
    pub commands_found: u32,
    #[serde(default, rename = "mechanicPoints")]
    pub mechanic_points: u32,
    pub garden: Option<ZooProfileTerminalGarden>,
    pub cards: ZooProfileTerminalCards,
    pub fusion: ZooProfileTerminalFusion,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileGoal {
    pub name: String,
    pub emoji: String,
    pub tier: String,
    #[serde(rename = "tierNumber")]
    pub tier_number: u32,
    pub target: u32,
    pub desc: String,
    pub count: u32,
    pub complete: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileSettings {
    #[serde(rename = "altTimestamp")]
    pub alt_timestamp: bool,
    #[serde(rename = "fastConfirmations")]
    pub fast_confirmations: bool,
    #[serde(rename = "showAnimalTotals")]
    pub show_animal_totals: bool,
    #[serde(rename = "disableRescueQuotes")]
    pub disable_rescue_quotes: bool,
    #[serde(rename = "disableNotifications")]
    pub disable_notifications: bool,
    #[serde(rename = "disableAutoRescues")]
    pub disable_auto_rescues: bool,
    #[serde(rename = "disableQuestNotifications")]
    pub disable_quest_notifications: bool,
    #[serde(rename = "disableCustomColor")]
    pub disable_custom_color: bool,
    #[serde(rename = "hideCosmetics")]
    pub hide_cosmetics: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileResponse {
    pub id: String,
    #[serde(rename = "userID")]
    pub user_id: String,
    #[serde(rename = "profileID")]
    pub profile_id: String,
    #[serde(rename = "selectedProfile")]
    pub selected_profile: String,
    pub profiles: Vec<String>,
    pub user: ZooProfileUser,
    pub name: String, // zoo name
    pub nickname: String,
    pub color: Option<String>, // hex color without #
    pub owner: bool,
    pub private: bool,
    #[serde(rename = "profileTheme")]
    pub profile_theme: String,
    pub score: u32,
    pub completion: f32,
    #[serde(rename = "uniqueAnimals")]
    pub unique_animals: ZooProfileUniqueAnimals,
    #[serde(rename = "totalAnimals")]
    pub total_animals: ZooProfileTotalAnimals,
    #[serde(rename = "pinnedAnimalScore")]
    pub pinned_animal_score: Option<ZooProfilePinnedAnimalScore>,
    #[serde(rename = "totalItems")]
    pub total_items: u32,
    #[serde(rename = "totalCosmetics")]
    pub total_cosmetics: u32,
    #[serde(rename = "totalTrophies")]
    pub total_trophies: u32,
    #[serde(rename = "totalLeaderXP")]
    pub total_leader_xp: u32,
    #[serde(rename = "unspentLeaderXP")]
    pub unspent_leader_xp: u32,
    #[serde(rename = "equippedRelics")]
    pub equipped_relics: Vec<String>,
    #[serde(rename = "equippedCosmetics")]
    pub equipped_cosmetics: Vec<String>,
    #[serde(rename = "equippedCosmetic")]
    pub equipped_cosmetic: Option<String>,
    #[serde(rename = "equippedLeader")]
    pub equipped_leader: Option<String>,
    #[serde(rename = "cosmeticIcon")]
    pub cosmetic_icon: Option<String>,
    pub notifications: u32,
    #[serde(rename = "autoRescues")]
    pub auto_rescues: u32,
    pub animals: Vec<ZooProfileAnimal>,
    pub items: Vec<ZooProfileItem>,
    pub relics: Vec<ZooProfileRelic>,
    pub cosmetics: Vec<ZooProfileCosmetic>,
    pub leaders: Vec<ZooProfileLeader>,
    pub quests: Vec<ZooProfileQuest>,
    pub quest: Option<ZooProfileActiveQuest>,
    pub curse: Option<ZooProfileCurse>,
    pub terminal: ZooProfileTerminal,
    pub stats: Vec<ZooProfileStat>,
    pub goals: Vec<ZooProfileGoal>,
    #[serde(rename = "goalTiers")]
    pub goal_tiers: u32,
    #[serde(rename = "goalsComplete")]
    pub goals_complete: u32,
    #[serde(rename = "extraData")]
    pub extra_data: Vec<Vec<serde_json::Value>>,
    pub settings: ZooProfileSettings,
    pub extra: serde_json::Value,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfilePinnedAnimalScore {
    pub red: u32,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileItem {
    pub name: String,
    pub amount: u32,
    pub emoji: String,
    pub highlight: bool,
    pub description: Option<String>,
    pub times_used: u32,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooProfileStat {
    pub name: String,
    pub value: u32,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooApiErrorResponse {
    #[serde(rename = "apiError")]
    pub api_error: bool,
    #[serde(rename = "internalError")]
    pub internal_error: bool,
    pub message: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ZooErrorResponse {
    pub name: String,
    pub msg: String,
    pub login: bool,
    pub invalid: bool,
    pub error: String,
}

pub fn profile_url(user_id: u64, profile: Option<&str>) -> String {
    if let Some(profile) = profile {
        format!("https://gdcolon.com/zoo/{}_{}", user_id, profile)
    } else {
        format!("https://gdcolon.com/zoo/{}", user_id)
    }
}

pub fn profile_api_url(user_id: u64, profile: Option<&str>) -> String {
    if let Some(profile) = profile {
        format!("https://gdcolon.com/zoo/api/profile/{}_{}", user_id, profile)
    } else {
        format!("https://gdcolon.com/zoo/api/profile/{}", user_id)
    }
}

#[derive(Debug, Clone)]
pub enum ZooProfileResult {
    Profile(Box<ZooProfileResponse>),
    Invalid(Box<ZooErrorResponse>),
    ApiError(Box<ZooApiErrorResponse>),
}

pub async fn fetch_zoo_profile(
    client: &reqwest::Client,
    user_id: u64,
    profile: Option<&str>,
) -> Result<ZooProfileResult> {
    let api_url = profile_api_url(user_id, profile);
    let response = client.get(&api_url).send().await?;
    let text = response.text().await?;
    match serde_json::from_str(&text) {
        Ok(profile) => Ok(ZooProfileResult::Profile(profile)),
        Err(e) => {
            if let Ok(error) = serde_json::from_str(&text) {
                return Ok(ZooProfileResult::Invalid(error));
            }
            if let Ok(error) = serde_json::from_str(&text) {
                return Ok(ZooProfileResult::ApiError(error));
            }
            Err(Error::new(e).context(format!("Response body: {}", text)))
        }
    }
}
