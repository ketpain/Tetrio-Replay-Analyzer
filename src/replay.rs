use crate::analysis::{DerivedStats, PlayerStats, StatTotals};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnalysisResult {
    pub round_stats: Vec<PlayerStats>,
    pub overall_stats: PlayerStats,
    pub winner: Option<String>,
}

impl AnalysisResult {
    pub fn is_empty(&self) -> bool {
        self.overall_stats.is_empty()
    }
}

#[derive(Debug, Deserialize)]
struct ReplayEnvelope {
    replay: Option<ReplayData>,
}

#[derive(Debug, Deserialize)]
struct ReplayData {
    #[serde(default)]
    leaderboard: Vec<LeaderboardEntry>,
    #[serde(default)]
    rounds: Vec<Vec<RoundPlayer>>,
}

#[derive(Debug, Deserialize)]
struct LeaderboardEntry {
    username: String,
    wins: u64,
}

#[derive(Debug, Deserialize)]
struct RoundPlayer {
    username: String,
    stats: ReplayStats,
}

#[derive(Debug, Deserialize)]
struct ReplayStats {
    pps: f32,
    apm: f32,
    #[serde(rename = "vsscore")]
    vs_score: f32,
}

pub fn analyze_replay_bytes(content: &[u8]) -> Result<AnalysisResult> {
    let envelope: ReplayEnvelope =
        serde_json::from_slice(content).context("failed to parse replay JSON")?;
    analyze_replay_envelope(envelope)
}

#[allow(dead_code)]
pub fn analyze_replay_str(content: &str) -> Result<AnalysisResult> {
    let envelope: ReplayEnvelope =
        serde_json::from_str(content).context("failed to parse replay JSON")?;
    analyze_replay_envelope(envelope)
}

fn analyze_replay_envelope(envelope: ReplayEnvelope) -> Result<AnalysisResult> {
    let replay = envelope.replay.context("unknown replay format")?;

    let winner = replay
        .leaderboard
        .iter()
        .max_by_key(|entry| entry.wins)
        .map(|entry| entry.username.clone());

    let mut round_stats = Vec::new();
    let mut totals_by_player: BTreeMap<String, (StatTotals, u32)> = BTreeMap::new();

    for round in replay.rounds {
        let mut round_result = PlayerStats::new();

        for player in round {
            let derived_stats =
                DerivedStats::from_base(player.stats.pps, player.stats.apm, player.stats.vs_score);

            round_result.insert(player.username.clone(), derived_stats.clone());

            let (totals, sample_count) = totals_by_player
                .entry(player.username)
                .or_insert_with(|| (StatTotals::default(), 0));
            totals.add(&derived_stats);
            *sample_count += 1;
        }

        if !round_result.is_empty() {
            round_stats.push(round_result);
        }
    }

    if totals_by_player.is_empty() {
        bail!("replay contained no player rounds");
    }

    let overall_stats = totals_by_player
        .into_iter()
        .map(|(username, (totals, sample_count))| (username, totals.average(sample_count)))
        .collect();

    Ok(AnalysisResult {
        round_stats,
        overall_stats,
        winner,
    })
}

pub fn combine_analysis_results<'a>(results: impl IntoIterator<Item = &'a AnalysisResult>) -> AnalysisResult {
    let mut totals_by_player: BTreeMap<String, (StatTotals, u32)> = BTreeMap::new();
    let mut win_totals: BTreeMap<String, u32> = BTreeMap::new();

    for result in results {
        for (player_name, player_stats) in &result.overall_stats {
            let (totals, sample_count) = totals_by_player
                .entry(player_name.clone())
                .or_insert_with(|| (StatTotals::default(), 0));
            totals.add(player_stats);
            *sample_count += 1;
        }

        if let Some(winner) = &result.winner {
            *win_totals.entry(winner.clone()).or_default() += 1;
        }
    }

    let overall_stats = totals_by_player
        .into_iter()
        .map(|(player_name, (totals, sample_count))| (player_name, totals.average(sample_count)))
        .collect();

    let winner = win_totals
        .into_iter()
        .max_by_key(|(_, total_wins)| *total_wins)
        .map(|(player_name, _)| player_name);

    AnalysisResult {
        round_stats: Vec::new(),
        overall_stats,
        winner,
    }
}

#[cfg(test)]
mod tests {
    use super::{analyze_replay_bytes, analyze_replay_str, combine_analysis_results, AnalysisResult};
    use crate::analysis::DerivedStats;
    use std::collections::BTreeMap;

    #[test]
    fn replay_parsing_aggregates_rounds() {
        let replay = r#"
        {
            "replay": {
                "leaderboard": [
                    {"username": "Alice", "wins": 2},
                    {"username": "Bob", "wins": 1}
                ],
                "rounds": [
                    [
                        {"username": "Alice", "stats": {"pps": 2.0, "apm": 120.0, "vsscore": 300.0}},
                        {"username": "Bob", "stats": {"pps": 1.5, "apm": 60.0, "vsscore": 180.0}}
                    ],
                    [
                        {"username": "Alice", "stats": {"pps": 2.2, "apm": 140.0, "vsscore": 320.0}},
                        {"username": "Bob", "stats": {"pps": 1.7, "apm": 80.0, "vsscore": 200.0}}
                    ]
                ]
            }
        }
        "#;

        let analysis = analyze_replay_str(replay).expect("replay should parse");

        assert_eq!(analysis.winner.as_deref(), Some("Alice"));
        assert_eq!(analysis.round_stats.len(), 2);

        let alice = analysis
            .overall_stats
            .get("Alice")
            .expect("Alice stats should exist");
        assert!((alice.pps - 2.1).abs() < 0.0001);
        assert!((alice.apm - 130.0).abs() < 0.0001);
        assert!((alice.vs_score - 310.0).abs() < 0.0001);
    }

    #[test]
    fn replay_bytes_parser_matches_string_parser() {
        let replay = r#"
        {
            "replay": {
                "leaderboard": [
                    {"username": "Alice", "wins": 2},
                    {"username": "Bob", "wins": 1}
                ],
                "rounds": [
                    [
                        {"username": "Alice", "stats": {"pps": 2.0, "apm": 120.0, "vsscore": 300.0}},
                        {"username": "Bob", "stats": {"pps": 1.5, "apm": 60.0, "vsscore": 180.0}}
                    ]
                ]
            }
        }
        "#;

        let from_str = analyze_replay_str(replay).expect("string replay should parse");
        let from_bytes = analyze_replay_bytes(replay.as_bytes()).expect("byte replay should parse");

        assert_eq!(from_bytes.winner, from_str.winner);
        assert_eq!(from_bytes.round_stats.len(), from_str.round_stats.len());
        assert_eq!(from_bytes.overall_stats.len(), from_str.overall_stats.len());
    }

    #[test]
    fn combining_results_matches_python_batch_behavior() {
        let first = AnalysisResult {
            round_stats: Vec::new(),
            overall_stats: BTreeMap::from([
                ("Alice".to_owned(), DerivedStats::from_base(2.0, 120.0, 300.0)),
                ("Bob".to_owned(), DerivedStats::from_base(1.5, 60.0, 180.0)),
            ]),
            winner: Some("Alice".to_owned()),
        };
        let second = AnalysisResult {
            round_stats: Vec::new(),
            overall_stats: BTreeMap::from([
                ("Alice".to_owned(), DerivedStats::from_base(2.4, 140.0, 320.0)),
                ("Bob".to_owned(), DerivedStats::from_base(1.7, 80.0, 200.0)),
            ]),
            winner: Some("Bob".to_owned()),
        };

        let combined = combine_analysis_results([&first, &second]);

        let alice = combined
            .overall_stats
            .get("Alice")
            .expect("Alice should exist in combined results");
        assert!((alice.pps - 2.2).abs() < 0.0001);
        assert!((alice.apm - 130.0).abs() < 0.0001);
        assert!((alice.vs_score - 310.0).abs() < 0.0001);
        assert_eq!(combined.winner.as_deref(), Some("Bob"));
        assert!(combined.round_stats.is_empty());
    }
}