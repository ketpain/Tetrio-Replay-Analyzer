use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type PlayerStats = BTreeMap<String, DerivedStats>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatKey {
    Pps,
    Apm,
    VsScore,
    App,
    DsPerPiece,
    DsPerSecond,
    GarbageEfficiency,
    DamagePotential,
}

impl StatKey {
    pub const ALL: [Self; 8] = [
        Self::Pps,
        Self::Apm,
        Self::VsScore,
        Self::App,
        Self::DsPerPiece,
        Self::DsPerSecond,
        Self::GarbageEfficiency,
        Self::DamagePotential,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Pps => "PPS",
            Self::Apm => "APM",
            Self::VsScore => "VS Score",
            Self::App => "APP",
            Self::DsPerPiece => "DS/Piece",
            Self::DsPerSecond => "DS/Second",
            Self::GarbageEfficiency => "Garbage Efficiency",
            Self::DamagePotential => "Damage Potential",
        }
    }

    pub fn range(self) -> (f32, f32) {
        match self {
            Self::Pps => (0.0, 4.0),
            Self::Apm => (0.0, 240.0),
            Self::VsScore => (0.0, 400.0),
            Self::App => (0.0, 1.0),
            Self::DsPerPiece => (0.0, 0.5),
            Self::DsPerSecond => (0.0, 1.0),
            Self::GarbageEfficiency => (0.0, 0.6),
            Self::DamagePotential => (0.0, 8.0),
        }
    }

    pub fn normalize(self, value: f32) -> f32 {
        let (min_value, max_value) = self.range();
        if (max_value - min_value).abs() < f32::EPSILON {
            return 0.5;
        }

        ((value - min_value) / (max_value - min_value)).clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DerivedStats {
    pub pps: f32,
    pub apm: f32,
    pub vs_score: f32,
    pub app: f32,
    pub ds_per_piece: f32,
    pub ds_per_second: f32,
    pub garbage_efficiency: f32,
    pub damage_potential: f32,
}

impl DerivedStats {
    pub fn from_base(pps: f32, apm: f32, vs_score: f32) -> Self {
        let app = calculate_app(apm, pps);
        let ds_per_piece = calculate_ds_per_piece(vs_score, apm, pps);
        let ds_per_second = calculate_ds_per_second(vs_score, apm);
        let garbage_efficiency = calculate_garbage_efficiency(app, ds_per_piece);
        let damage_potential = calculate_damage_potential(pps, app, garbage_efficiency);

        Self {
            pps,
            apm,
            vs_score,
            app,
            ds_per_piece,
            ds_per_second,
            garbage_efficiency,
            damage_potential,
        }
    }

    pub fn get(&self, key: StatKey) -> f32 {
        match key {
            StatKey::Pps => self.pps,
            StatKey::Apm => self.apm,
            StatKey::VsScore => self.vs_score,
            StatKey::App => self.app,
            StatKey::DsPerPiece => self.ds_per_piece,
            StatKey::DsPerSecond => self.ds_per_second,
            StatKey::GarbageEfficiency => self.garbage_efficiency,
            StatKey::DamagePotential => self.damage_potential,
        }
    }

    pub fn max_assign(&mut self, other: &Self) {
        self.pps = self.pps.max(other.pps);
        self.apm = self.apm.max(other.apm);
        self.vs_score = self.vs_score.max(other.vs_score);
        self.app = self.app.max(other.app);
        self.ds_per_piece = self.ds_per_piece.max(other.ds_per_piece);
        self.ds_per_second = self.ds_per_second.max(other.ds_per_second);
        self.garbage_efficiency = self.garbage_efficiency.max(other.garbage_efficiency);
        self.damage_potential = self.damage_potential.max(other.damage_potential);
    }
}

#[derive(Debug, Clone, Default)]
pub struct StatTotals {
    pps: f32,
    apm: f32,
    vs_score: f32,
    app: f32,
    ds_per_piece: f32,
    ds_per_second: f32,
    garbage_efficiency: f32,
    damage_potential: f32,
}

impl StatTotals {
    pub fn add(&mut self, stats: &DerivedStats) {
        self.pps += stats.pps;
        self.apm += stats.apm;
        self.vs_score += stats.vs_score;
        self.app += stats.app;
        self.ds_per_piece += stats.ds_per_piece;
        self.ds_per_second += stats.ds_per_second;
        self.garbage_efficiency += stats.garbage_efficiency;
        self.damage_potential += stats.damage_potential;
    }

    pub fn average(&self, sample_count: u32) -> DerivedStats {
        if sample_count == 0 {
            return DerivedStats::default();
        }

        let divisor = sample_count as f32;
        DerivedStats {
            pps: self.pps / divisor,
            apm: self.apm / divisor,
            vs_score: self.vs_score / divisor,
            app: self.app / divisor,
            ds_per_piece: self.ds_per_piece / divisor,
            ds_per_second: self.ds_per_second / divisor,
            garbage_efficiency: self.garbage_efficiency / divisor,
            damage_potential: self.damage_potential / divisor,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct MatchupRecord {
    pub wins: u32,
    pub losses: u32,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct MatchupSummary {
    pub ratio: f32,
    pub total_games: u32,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchupResult {
    Win,
    Loss,
}

#[derive(Debug, Clone)]
pub struct PlayerProfile {
    pub username: String,
    pub games_played: u32,
    totals: StatTotals,
    personal_bests: DerivedStats,
    matchups: BTreeMap<String, MatchupRecord>,
}

#[allow(dead_code)]
impl PlayerProfile {
    pub fn new(username: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            games_played: 0,
            totals: StatTotals::default(),
            personal_bests: DerivedStats::default(),
            matchups: BTreeMap::new(),
        }
    }

    pub fn add_game(&mut self, game_stats: &DerivedStats) {
        self.games_played += 1;
        self.totals.add(game_stats);
        self.personal_bests.max_assign(game_stats);
    }

    pub fn averages(&self) -> DerivedStats {
        self.totals.average(self.games_played)
    }

    pub fn personal_bests(&self) -> &DerivedStats {
        &self.personal_bests
    }

    pub fn add_matchup(&mut self, opponent: impl Into<String>, result: MatchupResult) {
        let record = self.matchups.entry(opponent.into()).or_default();
        match result {
            MatchupResult::Win => record.wins += 1,
            MatchupResult::Loss => record.losses += 1,
        }
    }

    pub fn matchup_history(&self) -> BTreeMap<String, MatchupSummary> {
        self.matchups
            .iter()
            .map(|(opponent, record)| {
                let total_games = record.wins + record.losses;
                let ratio = if total_games == 0 {
                    0.0
                } else {
                    record.wins as f32 / total_games as f32
                };
                (
                    opponent.clone(),
                    MatchupSummary {
                        ratio,
                        total_games,
                    },
                )
            })
            .collect()
    }
}

pub fn calculate_garbage_efficiency(app: f32, ds_per_piece: f32) -> f32 {
    if app <= 0.0 || ds_per_piece <= 0.0 {
        return 0.0;
    }

    (app * ds_per_piece) * 2.0
}

pub fn calculate_app(apm: f32, pps: f32) -> f32 {
    if pps <= 0.0 || apm <= 0.0 {
        return 0.0;
    }

    apm / (pps * 60.0)
}

pub fn calculate_ds_per_piece(vs_score: f32, apm: f32, pps: f32) -> f32 {
    if pps <= 0.0 || apm <= 0.0 {
        return 0.0;
    }

    let ds_per_second = (vs_score / 100.0) - (apm / 60.0);
    ds_per_second / pps
}

pub fn calculate_ds_per_second(vs_score: f32, apm: f32) -> f32 {
    (vs_score / 100.0) - (apm / 60.0)
}

pub fn calculate_damage_potential(pps: f32, app: f32, garbage_efficiency: f32) -> f32 {
    pps * (1.0 + app) * (1.0 + garbage_efficiency)
}

fn categorize(value: f32, thresholds: &[f32]) -> &'static str {
    const CATEGORIES: [&str; 7] = [
        "Low",
        "Below Average",
        "Average",
        "Above Average",
        "High",
        "Extremely High",
        "God-Tier",
    ];

    for (index, threshold) in thresholds.iter().enumerate() {
        if value < *threshold {
            return CATEGORIES[index];
        }
    }

    CATEGORIES[CATEGORIES.len() - 1]
}

pub fn analyze_play_style(player_profile: &PlayerProfile) -> String {
    let averages = player_profile.averages();
    let app = averages.app;
    let vs_apm_ratio = if averages.apm > 0.0 {
        averages.vs_score / averages.apm
    } else {
        0.0
    };
    let garbage_efficiency = averages.garbage_efficiency;
    let pps = averages.pps;

    let app_thresholds = [0.3, 0.45, 0.6, 0.75, 0.9];
    let garbage_efficiency_thresholds = [0.05, 0.10, 0.15, 0.20, 0.30];
    let vs_apm_thresholds = [1.6, 1.9, 2.0, 2.2, 2.5];
    let pps_thresholds = [1.0, 2.0, 2.5, 3.0, 4.0];

    let app_category = categorize(app, &app_thresholds);
    let garbage_efficiency_category = categorize(garbage_efficiency, &garbage_efficiency_thresholds);
    let vs_apm_category = categorize(vs_apm_ratio, &vs_apm_thresholds);
    let pps_category = categorize(pps, &pps_thresholds);

    let speed_descriptor = match pps_category {
        "Low" => "Very low-speed",
        "Below Average" => "Low-speed",
        "Average" => "Medium-speed",
        "Above Average" => "High-speed",
        "High" => "Very high-speed",
        "Extremely High" => "Extremely high-speed",
        _ => "God Tier-speed",
    };

    let attack_style = match app_category {
        "God-Tier" | "Extremely High" => "Highly efficient attacker",
        "High" | "Above Average" => "Efficient attacker",
        "Average" => "Balanced attacker",
        _ => "Inefficient attacker",
    };

    let aggressiveness = match vs_apm_category {
        "Low" | "Below Average" => match pps_category {
            "High" | "Extremely High" | "God-Tier" => "Highly offensive",
            "Above Average" | "Average" => "Offensive",
            _ => "Low-pressure player",
        },
        "Average" | "Above Average" => "Balanced",
        _ => match garbage_efficiency_category {
            "High" | "Extremely High" | "God-Tier" => "Defensive specialist",
            "Above Average" | "Average" => "Pressure-resistant",
            _ => "Defensive struggler",
        },
    };

    let garbage_style = match vs_apm_category {
        "God-Tier" | "Extremely High" | "High" => match garbage_efficiency_category {
            "God-Tier" | "Extremely High" | "High" => {
                "Exceptional garbage handler under extreme pressure"
            }
            "Above Average" | "Average" => "Competent garbage handler under high pressure",
            _ => "Struggles with efficiency under high pressure",
        },
        "Above Average" | "Average" => match garbage_efficiency_category {
            "God-Tier" | "Extremely High" | "High" => {
                "Highly efficient garbage handler under moderate pressure"
            }
            "Above Average" | "Average" => "Balanced garbage handling under moderate pressure",
            _ => "Inefficient garbage handler under moderate pressure",
        },
        _ => match garbage_efficiency_category {
            "God-Tier" | "Extremely High" | "High" => {
                "Highly efficient garbage handler with low incoming pressure"
            }
            "Above Average" | "Average" => {
                "Competent garbage handler with low incoming pressure"
            }
            _ => "Inefficient garbage handling, even under low pressure",
        },
    };

    let mut playstyle = format!(
        "{speed_descriptor}, {aggressiveness} player with {} capabilities. {garbage_style}.",
        attack_style.to_lowercase()
    );

    if matches!(vs_apm_category, "God-Tier" | "Extremely High" | "High")
        && matches!(app_category, "God-Tier" | "Extremely High" | "High")
    {
        playstyle.push_str(" Excels in high-pressure situations with efficient counterattacks.");
    } else if matches!(vs_apm_category, "Low" | "Below Average")
        && matches!(pps_category, "High" | "Extremely High" | "God-Tier")
    {
        playstyle.push_str(" Dominates through relentless offensive pressure.");
    } else if matches!(vs_apm_category, "God-Tier" | "Extremely High" | "High")
        && matches!(garbage_efficiency_category, "God-Tier" | "Extremely High" | "High")
    {
        playstyle.push_str(" Thrives on efficient downstacking under extreme pressure.");
    } else if matches!(vs_apm_category, "Low" | "Below Average")
        && matches!(app_category, "High" | "Extremely High" | "God-Tier")
    {
        playstyle.push_str(" Efficiently converts opportunities into strong attacks.");
    }

    playstyle
}

pub fn improvement_suggestions(player_profile: &PlayerProfile) -> Vec<String> {
    let averages = player_profile.averages();
    let app = averages.app;
    let vs_apm_ratio = if averages.apm > 0.0 {
        averages.vs_score / averages.apm
    } else {
        0.0
    };
    let garbage_efficiency = averages.garbage_efficiency;
    let pps = averages.pps;

    let app_thresholds = [0.3, 0.45, 0.6, 0.75, 0.9];
    let garbage_efficiency_thresholds = [0.05, 0.10, 0.15, 0.20, 0.30];
    let vs_apm_thresholds = [1.6, 1.9, 2.0, 2.2, 2.5];
    let pps_thresholds = [1.0, 2.0, 2.5, 3.0, 4.0];

    let app_category = categorize(app, &app_thresholds);
    let garbage_efficiency_category = categorize(garbage_efficiency, &garbage_efficiency_thresholds);
    let vs_apm_category = categorize(vs_apm_ratio, &vs_apm_thresholds);
    let pps_category = categorize(pps, &pps_thresholds);

    let mut suggestions = Vec::new();

    match pps_category {
        "God-Tier" => suggestions.push("Your speed is phenomenal. Focus on maintaining this level while optimizing efficiency, attack power, and consistency under varying pressure situations.".to_owned()),
        "Extremely High" | "High" => suggestions.push("Your speed is excellent. Work on consistency and efficiency at these high speeds.".to_owned()),
        "Above Average" | "Average" => suggestions.push("Your speed is good. Continue to improve by practicing finesse and efficient piece placement.".to_owned()),
        _ => suggestions.push("Focus on increasing your overall speed (PPS). Practice finesse and efficient piece placement.".to_owned()),
    }

    match garbage_efficiency_category {
        "God-Tier" => suggestions.push("Your garbage efficiency is outstanding. Maintain this level while optimizing other aspects of your game.".to_owned()),
        "Extremely High" | "High" => suggestions.push("Your garbage efficiency is very good. Fine-tune your downstacking for even better performance under pressure.".to_owned()),
        "Above Average" | "Average" => suggestions.push("Your garbage efficiency is decent. Practice more efficient downstacking techniques to improve further.".to_owned()),
        _ => suggestions.push("Work on improving your garbage efficiency. Focus on cleaner downstacking and better piece placement.".to_owned()),
    }

    match app_category {
        "God-Tier" => suggestions.push("Your attack efficiency is incredible. Focus on maintaining this level while adapting to different board states and opponent playstyles.".to_owned()),
        "Extremely High" | "High" => suggestions.push("Your attack efficiency is very good. Work on consistency and adapting to different situations.".to_owned()),
        "Above Average" | "Average" => suggestions.push("Your attack efficiency is solid. Practice more advanced attack techniques to increase your APP.".to_owned()),
        _ => suggestions.push("Improve your attack efficiency (APP). Practice building cleaner and executing attacks faster.".to_owned()),
    }

    if matches!(vs_apm_category, "High" | "Extremely High" | "God-Tier")
        && matches!(app_category, "High" | "Extremely High" | "God-Tier")
    {
        suggestions.push("You're effectively attacking while handling high pressure. Focus on maintaining this balance and look for opportunities to overwhelm opponents.".to_owned());
    } else if matches!(vs_apm_category, "High" | "Extremely High" | "God-Tier")
        && matches!(app_category, "Low" | "Below Average" | "Average")
    {
        suggestions.push("You're handling high pressure but could improve your attack efficiency. Work on building and executing attacks more effectively under pressure.".to_owned());
    } else if matches!(vs_apm_category, "Low" | "Below Average" | "Average")
        && matches!(app_category, "High" | "Extremely High" | "God-Tier")
    {
        suggestions.push("Your attacks are highly efficient, but you're not under much pressure. Practice maintaining this efficiency against stronger opponents or in faster-paced games.".to_owned());
    } else if matches!(vs_apm_category, "Low" | "Below Average" | "Average")
        && matches!(app_category, "Low" | "Below Average" | "Average")
    {
        suggestions.push("You're not under much pressure, but your attacks could be more efficient. Focus on improving your offensive capabilities to control the game better.".to_owned());
    }

    if matches!(vs_apm_category, "High" | "Extremely High" | "God-Tier")
        && matches!(garbage_efficiency_category, "High" | "Extremely High" | "God-Tier")
    {
        suggestions.push("You're excellently managing high amounts of garbage. Work on offensive strategies to reduce incoming attacks while maintaining this efficiency.".to_owned());
    } else if matches!(vs_apm_category, "High" | "Extremely High" | "God-Tier")
        && matches!(garbage_efficiency_category, "Low" | "Below Average" | "Average")
    {
        suggestions.push("You're under high pressure and could improve your garbage management. Focus on more efficient downstacking techniques.".to_owned());
    } else if matches!(vs_apm_category, "Low" | "Below Average" | "Average")
        && matches!(garbage_efficiency_category, "High" | "Extremely High" | "God-Tier")
    {
        suggestions.push("Your garbage efficiency is high, but you're not under much pressure. Prepare for handling higher pressure situations while maintaining this efficiency.".to_owned());
    } else if matches!(vs_apm_category, "Low" | "Below Average" | "Average")
        && matches!(garbage_efficiency_category, "Low" | "Below Average" | "Average")
    {
        suggestions.push("You're not under much pressure, but could improve garbage efficiency. Work on downstacking techniques to prepare for higher-pressure games.".to_owned());
    }

    if matches!(pps_category, "High" | "Extremely High" | "God-Tier")
        && matches!(app_category, "Low" | "Below Average")
    {
        suggestions.push("Your speed is excellent, but your attack efficiency could improve. Focus on converting your quick placements into more effective attacks.".to_owned());
    } else if matches!(app_category, "High" | "Extremely High" | "God-Tier")
        && matches!(pps_category, "Low" | "Below Average")
    {
        suggestions.push("Your attack efficiency is high, but overall speed is low. Work on increasing PPS while maintaining strong attack patterns.".to_owned());
    }

    suggestions.truncate(5);
    suggestions
}

#[cfg(test)]
mod tests {
    use super::{analyze_play_style, improvement_suggestions, DerivedStats, PlayerProfile, StatKey};

    #[test]
    fn derived_stats_match_python_formulas() {
        let stats = DerivedStats::from_base(2.0, 120.0, 300.0);

        assert!((stats.app - 1.0).abs() < 0.0001);
        assert!((stats.ds_per_second - 1.0).abs() < 0.0001);
        assert!((stats.ds_per_piece - 0.5).abs() < 0.0001);
        assert!((stats.garbage_efficiency - 1.0).abs() < 0.0001);
        assert!((stats.damage_potential - 8.0).abs() < 0.0001);
    }

    #[test]
    fn stat_normalization_clamps_values() {
        assert_eq!(StatKey::Pps.normalize(-1.0), 0.0);
        assert_eq!(StatKey::Pps.normalize(6.0), 1.0);
        assert!((StatKey::App.normalize(0.5) - 0.5).abs() < 0.0001);
    }

    #[test]
    fn profile_analysis_generates_text() {
        let mut profile = PlayerProfile::new("Alice");
        profile.add_game(&DerivedStats::from_base(2.1, 130.0, 280.0));

        let play_style = analyze_play_style(&profile);
        let suggestions = improvement_suggestions(&profile);

        assert!(play_style.contains("player"));
        assert!(!suggestions.is_empty());
        assert!(suggestions.len() <= 5);
    }
}