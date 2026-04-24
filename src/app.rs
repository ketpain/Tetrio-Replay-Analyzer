use crate::{
    analysis::{
        analyze_play_style, improvement_suggestions, DerivedStats, PlayerProfile, PlayerStats,
        StatKey,
    },
    cache,
    replay::{combine_analysis_results, AnalysisResult},
};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use macroquad::prelude::*;
use rfd::FileDialog;
use std::{
    collections::BTreeSet,
    f32::consts::{PI, TAU},
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread::{self, JoinHandle},
};

const BACKGROUND: Color = color_u8!(16, 20, 30, 255);
const PANEL: Color = color_u8!(30, 37, 51, 255);
const PANEL_ALT: Color = color_u8!(24, 31, 43, 255);
const PANEL_BORDER: Color = color_u8!(74, 89, 110, 255);
const TEXT_PRIMARY: Color = color_u8!(239, 244, 255, 255);
const TEXT_SECONDARY: Color = color_u8!(157, 174, 201, 255);
const BUTTON_IDLE: Color = color_u8!(47, 68, 96, 255);
const BUTTON_HOVER: Color = color_u8!(65, 91, 125, 255);
const BUTTON_DISABLED: Color = color_u8!(42, 49, 64, 255);
const SELECTED_ROW: Color = color_u8!(69, 103, 141, 255);
const BAR_BACKGROUND: Color = color_u8!(40, 49, 66, 255);
const BAR_FILL: Color = color_u8!(91, 197, 177, 255);
const WINNER: Color = color_u8!(85, 180, 120, 255);
const INPUT_BACKGROUND: Color = color_u8!(36, 45, 61, 255);
const INPUT_FOCUSED: Color = color_u8!(54, 73, 102, 255);
const OVERLAY: Color = color_u8!(8, 10, 16, 190);
const FILE_ROW_HEIGHT: f32 = 36.0;
const FILE_SCROLL_STEP: f32 = 42.0;
const PROFILE_SCROLL_STEP: f32 = 34.0;
const MAX_WHEEL_SCROLL_STEPS: f32 = 2.0;
const RADAR_RING_COUNT: usize = 5;
const ANALYZER_CARD_HEADER_HEIGHT: f32 = 38.0;
const ANALYZER_CARD_PADDING: f32 = 12.0;
const ANALYZER_CARD_RESIZE_HANDLE: f32 = 18.0;
const ANALYZER_CARD_MIN_TABLE_WIDTH: f32 = 440.0;
const ANALYZER_CARD_MIN_TABLE_HEIGHT: f32 = 220.0;
const ANALYZER_CARD_MIN_RADAR_WIDTH: f32 = 360.0;
const ANALYZER_CARD_MIN_RADAR_HEIGHT: f32 = 230.0;
const ANALYZER_CARD_MIN_PROFILE_WIDTH: f32 = 360.0;
const ANALYZER_CARD_MIN_PROFILE_HEIGHT: f32 = 190.0;

static UI_FONT: OnceLock<Mutex<Option<Font>>> = OnceLock::new();

const CORE_RADAR_METRICS: [(StatKey, &str); 7] = [
    (StatKey::Pps, "PPS"),
    (StatKey::Apm, "APM"),
    (StatKey::VsScore, "VS Score"),
    (StatKey::App, "APP"),
    (StatKey::DsPerPiece, "DS/Piece"),
    (StatKey::DsPerSecond, "DS/Second"),
    (StatKey::GarbageEfficiency, "Garbage Eff."),
];

const ATTACK_DEFENSE_METRICS: [(StatKey, &str); 4] = [
    (StatKey::App, "Attack"),
    (StatKey::GarbageEfficiency, "Defense"),
    (StatKey::Pps, "Speed"),
    (StatKey::DamagePotential, "Damage"),
];

#[derive(Clone)]
enum AnalysisSource {
    SingleFile(usize),
    MultiFile(Vec<usize>),
    ManualInput,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TextField {
    PlayerFilter,
    ManualPps,
    ManualApm,
    ManualVsScore,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScrollbarTarget {
    ReplayList,
    Profiles,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AnalyzerCardKind {
    WinnerTable,
    RadarCharts,
    PlayerProfiles,
}

#[derive(Clone)]
struct ManualInputState {
    pps: String,
    apm: String,
    vs_score: String,
    error_message: Option<String>,
}

impl ManualInputState {
    fn new() -> Self {
        Self {
            pps: String::new(),
            apm: String::new(),
            vs_score: String::new(),
            error_message: None,
        }
    }
}

enum ProgressTaskMode {
    AnalyzeSelection { selected_indices: Vec<usize> },
    RebuildCache,
}

struct ReplayJob {
    path: PathBuf,
    file_name: String,
    force_reprocess: bool,
}

struct WorkerResult {
    path: PathBuf,
    file_name: String,
    outcome: Result<cache::ProcessedReplay, String>,
}

enum WorkerEvent {
    FileDone(WorkerResult),
    Finished,
}

struct BackgroundProcessor {
    event_rx: Receiver<WorkerEvent>,
    cancel_flag: Arc<AtomicBool>,
    completion_thread: Option<JoinHandle<()>>,
}

struct ProgressTask {
    mode: ProgressTaskMode,
    label: String,
    total_files: usize,
    completed_files: usize,
    current_file: String,
    successful_files: usize,
    successes: Vec<(PathBuf, AnalysisResult)>,
    failed_files: Vec<String>,
    retained_paths: Option<BTreeSet<PathBuf>>,
    background: BackgroundProcessor,
}

struct DisplayState {
    stats: PlayerStats,
    winner: Option<String>,
    round_label: String,
}

struct ProfileCardContent {
    player_name: String,
    play_style: String,
    suggestions: Vec<String>,
    height: f32,
}

#[derive(Clone, Copy)]
struct ActiveScrollbarDrag {
    target: ScrollbarTarget,
    grab_offset: f32,
}

#[derive(Clone, Copy)]
enum AnalyzerCardInteractionMode {
    Drag { grab_offset: Vec2 },
    Resize {
        right_offset: f32,
        bottom_offset: f32,
    },
}

#[derive(Clone, Copy)]
struct ActiveAnalyzerCardInteraction {
    card: AnalyzerCardKind,
    mode: AnalyzerCardInteractionMode,
}

#[derive(Clone, Copy, Default)]
struct ScrollState {
    current: f32,
    target: f32,
}

impl ScrollState {
    fn reset(&mut self) {
        self.current = 0.0;
        self.target = 0.0;
    }

    fn sync_target(&mut self) {
        self.target = self.current;
    }
}

#[derive(Clone, Copy)]
struct ScrollbarMetrics {
    track_rect: Rect,
    thumb_rect: Rect,
    max_scroll: f32,
    thumb_range: f32,
}

pub struct App {
    cache_dir: PathBuf,
    current_folder: Option<PathBuf>,
    replay_files: Vec<PathBuf>,
    selected_files: BTreeSet<usize>,
    current_analysis: Option<AnalysisResult>,
    current_source: Option<AnalysisSource>,
    player_filter: String,
    current_round_index: usize,
    active_text_field: Option<TextField>,
    manual_input: Option<ManualInputState>,
    progress_task: Option<ProgressTask>,
    status_message: String,
    file_scroll: ScrollState,
    profile_scroll: ScrollState,
    active_scrollbar_drag: Option<ActiveScrollbarDrag>,
    winner_table_card: Rect,
    radar_charts_card: Rect,
    player_profiles_card: Rect,
    analyzer_card_order: [AnalyzerCardKind; 3],
    active_analyzer_card: Option<ActiveAnalyzerCardInteraction>,
}

impl App {
    pub fn new() -> Self {
        Self {
            cache_dir: cache::default_cache_dir(),
            current_folder: None,
            replay_files: Vec::new(),
            selected_files: BTreeSet::new(),
            current_analysis: None,
            current_source: None,
            player_filter: String::new(),
            current_round_index: 0,
            active_text_field: None,
            manual_input: None,
            progress_task: None,
            status_message: "Select a folder containing .ttrm replays to begin.".to_owned(),
            file_scroll: ScrollState::default(),
            profile_scroll: ScrollState::default(),
            active_scrollbar_drag: None,
            winner_table_card: Rect::new(0.0, 0.0, 1.0, 0.34),
            radar_charts_card: Rect::new(0.0, 0.36, 1.0, 0.30),
            player_profiles_card: Rect::new(0.0, 0.68, 1.0, 0.32),
            analyzer_card_order: [
                AnalyzerCardKind::WinnerTable,
                AnalyzerCardKind::RadarCharts,
                AnalyzerCardKind::PlayerProfiles,
            ],
            active_analyzer_card: None,
        }
    }

    pub fn frame(&mut self) {
        if !is_mouse_button_down(MouseButton::Left) {
            self.active_scrollbar_drag = None;
            self.active_analyzer_card = None;
        }

        self.handle_keyboard_input();
        self.tick_progress_task();

        clear_background(BACKGROUND);

        let left_panel = Rect::new(24.0, 24.0, 360.0, screen_height() - 48.0);
        let right_panel = Rect::new(408.0, 24.0, screen_width() - 432.0, screen_height() - 48.0);

        self.draw_panel(left_panel, "Replay Browser");
        self.draw_panel(right_panel, "Analyzer");

        self.draw_left_panel(left_panel);
        self.draw_right_panel(right_panel);

        if self.manual_input.is_some() {
            self.draw_manual_input_modal();
        }

        if self.progress_task.is_some() {
            self.draw_progress_overlay();
        }
    }

    fn draw_left_panel(&mut self, rect: Rect) {
        let margin = 18.0;
        let button_height = 38.0;
        let button_gap = 12.0;
        let button_width = (rect.w - margin * 2.0 - button_gap) / 2.0;
        let mut cursor_y = rect.y + 62.0;

        if self.button(
            Rect::new(rect.x + margin, cursor_y, button_width, button_height),
            "Select Folder",
            true,
        ) {
            if let Some(folder_path) = FileDialog::new().pick_folder() {
                self.set_folder(folder_path);
            }
        }

        if self.button(
            Rect::new(
                rect.x + margin + button_width + button_gap,
                cursor_y,
                button_width,
                button_height,
            ),
            "Refresh",
            self.current_folder.is_some(),
        ) {
            self.refresh_files();
        }

        cursor_y += button_height + 10.0;

        if self.button(
            Rect::new(rect.x + margin, cursor_y, button_width, button_height),
            "Analyze Selected",
            !self.selected_files.is_empty(),
        ) {
            self.start_analysis_task(self.selected_indices());
        }

        if self.button(
            Rect::new(
                rect.x + margin + button_width + button_gap,
                cursor_y,
                button_width,
                button_height,
            ),
            "Rebuild Cache",
            !self.replay_files.is_empty(),
        ) {
            self.start_cache_rebuild_task();
        }

        cursor_y += button_height + 10.0;

        if self.button(
            Rect::new(rect.x + margin, cursor_y, rect.w - margin * 2.0, button_height),
            "Manual Input",
            true,
        ) {
            self.open_manual_input();
        }

        cursor_y += button_height + 18.0;

        draw_text_ex(
            "Folder",
            rect.x + margin,
            cursor_y,
            TextParams {
                font_size: 22,
                color: TEXT_PRIMARY,
                ..Default::default()
            },
        );
        cursor_y += 24.0;

        let folder_label = self
            .current_folder
            .as_ref()
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_else(|| "No folder selected".to_owned());
        cursor_y = draw_wrapped_text(
            &folder_label,
            rect.x + margin,
            cursor_y,
            rect.w - margin * 2.0,
            18,
            22.0,
            TEXT_SECONDARY,
        );
        cursor_y += 8.0;

        let selection_label = if self.selected_files.is_empty() {
            "Selected: none".to_owned()
        } else {
            format!("Selected: {} replay(s)", self.selected_files.len())
        };
        cursor_y = draw_wrapped_text(
            &selection_label,
            rect.x + margin,
            cursor_y,
            rect.w - margin * 2.0,
            18,
            22.0,
            TEXT_SECONDARY,
        );
        cursor_y += 4.0;

        cursor_y = draw_wrapped_text(
            "Tip: Ctrl+Click to build a multi-file selection, or single-click a replay to analyze it immediately.",
            rect.x + margin,
            cursor_y,
            rect.w - margin * 2.0,
            16,
            20.0,
            TEXT_SECONDARY,
        );
        cursor_y += 8.0;

        draw_text_ex(
            "Status",
            rect.x + margin,
            cursor_y,
            TextParams {
                font_size: 22,
                color: TEXT_PRIMARY,
                ..Default::default()
            },
        );
        cursor_y += 24.0;
        cursor_y = draw_wrapped_text(
            &self.status_message,
            rect.x + margin,
            cursor_y,
            rect.w - margin * 2.0,
            18,
            22.0,
            TEXT_SECONDARY,
        );
        cursor_y += 14.0;

        let list_rect = Rect::new(
            rect.x + margin,
            cursor_y,
            rect.w - margin * 2.0,
            rect.y + rect.h - cursor_y - margin,
        );
        self.draw_file_list(list_rect);
    }

    fn draw_right_panel(&mut self, rect: Rect) {
        let margin = 18.0;
        let mut cursor_y = rect.y + 62.0;
        let title = self.analysis_title();

        draw_text_ex(
            &title,
            rect.x + margin,
            cursor_y,
            TextParams {
                font_size: 30,
                color: TEXT_PRIMARY,
                ..Default::default()
            },
        );
        cursor_y += 34.0;

        let summary = self.analysis_summary();
        cursor_y = draw_wrapped_text(
            &summary,
            rect.x + margin,
            cursor_y,
            rect.w - margin * 2.0,
            18,
            22.0,
            if self.current_analysis.is_some() {
                WINNER
            } else {
                TEXT_SECONDARY
            },
        );
        cursor_y += 8.0;

        let controls_rect = Rect::new(rect.x + margin, cursor_y, rect.w - margin * 2.0, 58.0);
        self.draw_controls_bar(controls_rect);
        cursor_y = controls_rect.y + controls_rect.h + 16.0;

        let Some(display) = self.current_display_state() else {
            draw_wrapped_text(
                "Choose a replay to analyze, or open Manual Input to explore a custom stat line. The current Rust version now supports round selection, player filtering, batch analysis, cache rebuilds, radar charts, and the winner table.",
                rect.x + margin,
                cursor_y + 10.0,
                rect.w - margin * 2.0,
                24,
                28.0,
                TEXT_SECONDARY,
            );
            return;
        };

        let workspace = Rect::new(
            rect.x + margin,
            cursor_y,
            (rect.w - margin * 2.0).max(0.0),
            (rect.y + rect.h - cursor_y - margin).max(0.0),
        );
        let profile_stats = self.filtered_overall_stats();

        self.update_analyzer_card_interaction(workspace);

        for card in self.analyzer_card_order {
            let card_rect = self.analyzer_card_actual_rect(card, workspace);
            let content_rect = self.draw_analyzer_card(card, card_rect);

            match card {
                AnalyzerCardKind::WinnerTable => {
                    self.draw_stats_table(content_rect, &display.stats, display.winner.as_deref());
                }
                AnalyzerCardKind::RadarCharts => {
                    self.draw_radar_charts(content_rect, &display.stats);
                }
                AnalyzerCardKind::PlayerProfiles => {
                    self.draw_profile_cards(content_rect, &profile_stats);
                }
            }
        }
    }

    fn draw_controls_bar(&mut self, rect: Rect) {
        draw_rectangle(rect.x, rect.y, rect.w, rect.h, PANEL_ALT);
        draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, PANEL_BORDER);

        let label_font_size = 18;
        let label_gap = 16.0;
        let section_gap = 24.0;
        let selector_button_width = 34.0;
        let selector_box_width = 124.0;
        let label_baseline_y =
            rect.y + rect.h / 2.0 + measure_text("Ag", None, label_font_size, 1.0).height / 2.5;

        let filter_label_x = rect.x + 14.0;
        let filter_label_width = measure_text("Player Filter", None, label_font_size, 1.0).width;
        draw_text_ex(
            "Player Filter",
            filter_label_x,
            label_baseline_y,
            TextParams {
                font_size: label_font_size,
                color: TEXT_PRIMARY,
                ..Default::default()
            },
        );

        let filter_rect = Rect::new(
            filter_label_x + filter_label_width + label_gap,
            rect.y + 10.0,
            240.0,
            36.0,
        );
        if self.text_input_box(
            filter_rect,
            &self.player_filter,
            "Filter players...",
            self.active_text_field == Some(TextField::PlayerFilter),
            true,
            false,
        ) {
            self.active_text_field = Some(TextField::PlayerFilter);
        }

        let round_count = self
            .current_analysis
            .as_ref()
            .map(|analysis| analysis.round_stats.len())
            .unwrap_or(0);
        let round_label = self.current_round_label();
        let round_label_x = filter_rect.x + filter_rect.w + section_gap;
        let round_label_width = measure_text("Round View", None, label_font_size, 1.0).width;
        let round_prev_button_x = round_label_x + round_label_width + label_gap;

        draw_text_ex(
            "Round View",
            round_label_x,
            label_baseline_y,
            TextParams {
                font_size: label_font_size,
                color: TEXT_PRIMARY,
                ..Default::default()
            },
        );

        if self.button(
            Rect::new(round_prev_button_x, rect.y + 10.0, selector_button_width, 36.0),
            "<",
            round_count > 0 && self.current_round_index > 0,
        ) {
            self.current_round_index -= 1;
        }

        let round_box = Rect::new(
            round_prev_button_x + selector_button_width + 8.0,
            rect.y + 10.0,
            selector_box_width,
            36.0,
        );
        draw_rectangle(round_box.x, round_box.y, round_box.w, round_box.h, INPUT_BACKGROUND);
        draw_rectangle_lines(round_box.x, round_box.y, round_box.w, round_box.h, 1.0, PANEL_BORDER);
        draw_text_centered(&round_label, round_box, 18, TEXT_PRIMARY);

        if self.button(
            Rect::new(
                round_box.x + round_box.w + 8.0,
                rect.y + 10.0,
                selector_button_width,
                36.0,
            ),
            ">",
            round_count > 0 && self.current_round_index < round_count,
        ) {
            self.current_round_index += 1;
        }

        let selected_hint = format!("Selection: {}", self.selected_files.len());
        draw_text_ex(
            &selected_hint,
            rect.x + rect.w - 134.0,
            rect.y + 31.0,
            TextParams {
                font_size: 18,
                color: TEXT_SECONDARY,
                ..Default::default()
            },
        );
    }

    fn draw_file_list(&mut self, rect: Rect) {
        draw_rectangle(rect.x, rect.y, rect.w, rect.h, PANEL_ALT);
        draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, PANEL_BORDER);

        if self.replay_files.is_empty() {
            draw_wrapped_text(
                "No .ttrm replay files found in the selected folder.",
                rect.x + 14.0,
                rect.y + 28.0,
                rect.w - 28.0,
                18,
                24.0,
                TEXT_SECONDARY,
            );
            return;
        }

        let gap = 8.0;
        let scrollbar_width = 10.0;
        let viewport = Rect::new(
            rect.x + gap,
            rect.y + gap,
            (rect.w - gap * 3.0 - scrollbar_width).max(0.0),
            (rect.h - gap * 2.0).max(0.0),
        );
        let content_height = self.replay_files.len() as f32 * FILE_ROW_HEIGHT;
        let input_blocked = self.input_blocked();
        update_scroll_state(
            &mut self.file_scroll,
            viewport,
            content_height,
            FILE_SCROLL_STEP,
            input_blocked,
        );

        let ctrl_pressed = is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl);
        let mut clicked_index = None;

        with_scissor(viewport, || {
            for (index, replay_path) in self.replay_files.iter().enumerate() {
                let y = viewport.y + index as f32 * FILE_ROW_HEIGHT - self.file_scroll.current;
                if y + FILE_ROW_HEIGHT < viewport.y || y > viewport.y + viewport.h {
                    continue;
                }

                let row_rect = Rect::new(viewport.x, y, viewport.w, FILE_ROW_HEIGHT - 4.0);
                let is_selected = self.selected_files.contains(&index);
                let label = replay_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Replay");

                if self.selectable_row(row_rect, label, is_selected) {
                    clicked_index = Some(index);
                }
            }
        });

        let scrollbar_rect = Rect::new(
            rect.x + rect.w - gap - scrollbar_width,
            viewport.y,
            scrollbar_width,
            viewport.h,
        );
        let (file_scroll, scrollbar_interacted) = self.draw_scrollbar(
            ScrollbarTarget::ReplayList,
            scrollbar_rect,
            content_height.max(viewport.h),
            viewport.h,
            self.file_scroll.current,
        );
        self.file_scroll.current = file_scroll;
        if scrollbar_interacted {
            self.file_scroll.sync_target();
        }

        if let Some(index) = clicked_index {
            if ctrl_pressed {
                if !self.selected_files.insert(index) {
                    self.selected_files.remove(&index);
                }
                if self.selected_files.is_empty() && !matches!(self.current_source, Some(AnalysisSource::ManualInput)) {
                    self.clear_analysis();
                }
            } else {
                self.selected_files.clear();
                self.selected_files.insert(index);
                self.start_analysis_task(vec![index]);
            }
        }
    }

    fn draw_stats_table(&self, rect: Rect, stats_by_player: &PlayerStats, winner: Option<&str>) {
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return;
        }

        let table_rect = rect;
        draw_rectangle(table_rect.x, table_rect.y, table_rect.w, table_rect.h, PANEL_ALT);
        draw_rectangle_lines(table_rect.x, table_rect.y, table_rect.w, table_rect.h, 1.0, PANEL_BORDER);

        if stats_by_player.is_empty() {
            draw_wrapped_text(
                "No players match the current view or filter.",
                table_rect.x + 16.0,
                table_rect.y + 32.0,
                table_rect.w - 32.0,
                18,
                24.0,
                TEXT_SECONDARY,
            );
            return;
        }

        let players: Vec<_> = stats_by_player.iter().collect();
        let colors = distinct_player_colors(players.len());
        let stat_rows = StatKey::ALL;
        let row_count = stat_rows.len() + 2;
        let row_height = (table_rect.h / row_count as f32).min(34.0).max(0.0);
        let label_width = 170.0;
        let cell_width = ((table_rect.w - label_width) / players.len() as f32).max(120.0);
        let font_size = 16;

        with_scissor(table_rect, || {
            self.draw_table_cell(
                Rect::new(table_rect.x, table_rect.y, label_width, row_height),
                "Stat",
                PANEL,
                TEXT_PRIMARY,
                font_size,
            );

            for (column, ((player_name, _), color)) in players.iter().zip(colors.iter()).enumerate() {
                self.draw_table_cell(
                    Rect::new(
                        table_rect.x + label_width + column as f32 * cell_width,
                        table_rect.y,
                        cell_width,
                        row_height,
                    ),
                    player_name,
                    *color,
                    TEXT_PRIMARY,
                    font_size,
                );
            }

            for (row, key) in stat_rows.iter().enumerate() {
                let y = table_rect.y + (row as f32 + 1.0) * row_height;
                self.draw_table_cell(
                    Rect::new(table_rect.x, y, label_width, row_height),
                    key.label(),
                    PANEL,
                    TEXT_PRIMARY,
                    font_size,
                );

                for (column, (_, player_stats)) in players.iter().enumerate() {
                    let cell_rect = Rect::new(
                        table_rect.x + label_width + column as f32 * cell_width,
                        y,
                        cell_width,
                        row_height,
                    );
                    let text = format!("{:.2}", player_stats.get(*key));

                    self.draw_table_cell(cell_rect, &text, PANEL_ALT, TEXT_PRIMARY, font_size);
                }
            }

            let winner_row_y = table_rect.y + (stat_rows.len() as f32 + 1.0) * row_height;
            self.draw_table_cell(
                Rect::new(table_rect.x, winner_row_y, label_width, row_height),
                "WINNER",
                PANEL,
                TEXT_PRIMARY,
                font_size,
            );
            for (column, (player_name, _)) in players.iter().enumerate() {
                let cell_rect = Rect::new(
                    table_rect.x + label_width + column as f32 * cell_width,
                    winner_row_y,
                    cell_width,
                    row_height,
                );
                let (text, background) = if winner == Some(player_name.as_str()) {
                    ("WINNER", WINNER)
                } else {
                    ("", PANEL_ALT)
                };
                self.draw_table_cell(cell_rect, text, background, TEXT_PRIMARY, font_size);
            }
        });
    }

    fn draw_table_cell(&self, rect: Rect, text: &str, background: Color, foreground: Color, font_size: u16) {
        draw_rectangle(rect.x, rect.y, rect.w, rect.h, background);
        draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, PANEL_BORDER);
        draw_text_centered(text, rect, font_size, foreground);
    }

    fn draw_radar_charts(&self, rect: Rect, stats_by_player: &PlayerStats) {
        let charts_rect = rect;
        if charts_rect.w <= 0.0 || charts_rect.h <= 0.0 {
            return;
        }

        let gap = 14.0;
        let chart_width = (charts_rect.w - gap) / 2.0;
        let left_chart = Rect::new(charts_rect.x, charts_rect.y, chart_width, charts_rect.h);
        let right_chart = Rect::new(charts_rect.x + chart_width + gap, charts_rect.y, chart_width, charts_rect.h);

        self.draw_radar_chart(left_chart, "Core Stats", stats_by_player, &CORE_RADAR_METRICS);
        self.draw_radar_chart(right_chart, "Attack / Defense / Speed", stats_by_player, &ATTACK_DEFENSE_METRICS);
    }

    fn draw_radar_chart(
        &self,
        rect: Rect,
        title: &str,
        stats_by_player: &PlayerStats,
        metrics: &[(StatKey, &str)],
    ) {
        draw_rectangle(rect.x, rect.y, rect.w, rect.h, PANEL_ALT);
        draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, PANEL_BORDER);
        draw_text_ex(
            title,
            rect.x + 14.0,
            rect.y + 24.0,
            TextParams {
                font_size: 20,
                color: TEXT_PRIMARY,
                ..Default::default()
            },
        );

        if rect.w <= 0.0 || rect.h <= 0.0 {
            return;
        }

        if stats_by_player.is_empty() {
            draw_wrapped_text(
                "No visible players for this chart.",
                rect.x + 14.0,
                rect.y + 50.0,
                rect.w - 28.0,
                18,
                24.0,
                TEXT_SECONDARY,
            );
            return;
        }

        let colors = distinct_player_colors(stats_by_player.len());
        let content_rect = Rect::new(
            rect.x + 1.0,
            rect.y + 1.0,
            (rect.w - 2.0).max(0.0),
            (rect.h - 2.0).max(0.0),
        );

        with_scissor(content_rect, || {
            let legend_space = 44.0_f32.min((rect.h * 0.25).max(24.0));
            let label_padding = 32.0;
            let usable_chart_height = (rect.h - legend_space - 54.0).max(0.0);
            let usable_chart_width = (rect.w - label_padding * 2.0).max(0.0);
            let radius = (usable_chart_width.min(usable_chart_height) / 2.0).max(8.0);
            let center = vec2(
                rect.x + rect.w / 2.0,
                rect.y + 42.0 + usable_chart_height / 2.0,
            );

            for ring in 1..=RADAR_RING_COUNT {
                let ring_radius = radius * ring as f32 / RADAR_RING_COUNT as f32;
                let points = radar_points(center, ring_radius, metrics.len(), |_| 1.0);
                draw_polyline(&points, PANEL_BORDER, 1.0);
            }

            let axis_points = radar_points(center, radius, metrics.len(), |_| 1.0);
            for point in &axis_points {
                draw_line(center.x, center.y, point.x, point.y, 1.0, PANEL_BORDER);
            }

            for (index, (_, label)) in metrics.iter().enumerate() {
                let angle = -PI / 2.0 + TAU * index as f32 / metrics.len() as f32;
                let label_pos = vec2(
                    center.x + (radius + 14.0) * angle.cos(),
                    center.y + (radius + 14.0) * angle.sin(),
                );
                let measured = measure_text(label, None, 16, 1.0);
                draw_text_ex(
                    label,
                    label_pos.x - measured.width / 2.0,
                    label_pos.y + measured.height / 2.0,
                    TextParams {
                        font_size: 16,
                        color: TEXT_SECONDARY,
                        ..Default::default()
                    },
                );
            }

            for ((_, player_stats), color) in stats_by_player.iter().zip(colors.iter()) {
                let points = radar_points(center, radius, metrics.len(), |index| {
                    metrics[index].0.normalize(player_stats.get(metrics[index].0))
                });
                draw_polyline(&points, *color, 2.0);
                for point in points {
                    draw_circle(point.x, point.y, 3.2, *color);
                }
            }

            let mut legend_x = rect.x + 14.0;
            let legend_y = rect.y + rect.h - 18.0;
            for ((player_name, _), color) in stats_by_player.iter().zip(colors.iter()) {
                draw_rectangle(legend_x, legend_y - 12.0, 12.0, 12.0, *color);
                draw_text_ex(
                    player_name,
                    legend_x + 18.0,
                    legend_y,
                    TextParams {
                        font_size: 16,
                        color: TEXT_PRIMARY,
                        ..Default::default()
                    },
                );
                legend_x += measure_text(player_name, None, 16, 1.0).width + 40.0;
            }
        });
    }

    fn draw_profile_cards(&mut self, rect: Rect, stats_by_player: &PlayerStats) {
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return;
        }

        let cards_rect = rect;
        draw_rectangle(cards_rect.x, cards_rect.y, cards_rect.w, cards_rect.h, PANEL_ALT);
        draw_rectangle_lines(cards_rect.x, cards_rect.y, cards_rect.w, cards_rect.h, 1.0, PANEL_BORDER);

        if stats_by_player.is_empty() {
            draw_wrapped_text(
                "No player profiles are visible under the current filter.",
                cards_rect.x + 14.0,
                cards_rect.y + 28.0,
                cards_rect.w - 28.0,
                18,
                24.0,
                TEXT_SECONDARY,
            );
            return;
        }

        let columns = if stats_by_player.len() <= 1 { 1 } else { 2 };
        let gap = 14.0;
        let scrollbar_width = 10.0;
        let viewport = Rect::new(
            cards_rect.x + gap,
            cards_rect.y + gap,
            (cards_rect.w - gap * 3.0 - scrollbar_width).max(0.0),
            (cards_rect.h - gap * 2.0).max(0.0),
        );
        let card_width = if columns == 1 {
            viewport.w
        } else {
            (viewport.w - gap * (columns as f32 - 1.0)) / columns as f32
        };
        let profile_cards: Vec<ProfileCardContent> = stats_by_player
            .iter()
            .map(|(player_name, player_stats)| build_profile_card_content(player_name, player_stats, card_width))
            .collect();

        let rows = ((profile_cards.len() as f32) / columns as f32).ceil() as usize;
        let mut row_heights = Vec::with_capacity(rows);
        for row in 0..rows {
            let start = row * columns;
            let end = (start + columns).min(profile_cards.len());
            let row_height = profile_cards[start..end]
                .iter()
                .fold(0.0_f32, |max_height, card| max_height.max(card.height));
            row_heights.push(row_height);
        }

        let total_content_height = row_heights.iter().sum::<f32>() + gap * rows.saturating_sub(1) as f32;
        let input_blocked = self.input_blocked();
        update_scroll_state(
            &mut self.profile_scroll,
            viewport,
            total_content_height,
            PROFILE_SCROLL_STEP,
            input_blocked,
        );

        let mut row_top = viewport.y - self.profile_scroll.current;
        with_scissor(viewport, || {
            for (row, row_height) in row_heights.iter().enumerate() {
                for column in 0..columns {
                    let index = row * columns + column;
                    let Some(card) = profile_cards.get(index) else {
                        continue;
                    };

                    let card_rect = Rect::new(
                        viewport.x + column as f32 * (card_width + gap),
                        row_top,
                        card_width,
                        card.height,
                    );
                    self.draw_profile_card(card_rect, card);
                }

                row_top += *row_height + gap;
            }
        });

        let scrollbar_rect = Rect::new(
            cards_rect.x + cards_rect.w - gap - scrollbar_width,
            viewport.y,
            scrollbar_width,
            viewport.h,
        );
        let (profile_scroll, scrollbar_interacted) = self.draw_scrollbar(
            ScrollbarTarget::Profiles,
            scrollbar_rect,
            total_content_height.max(viewport.h),
            viewport.h,
            self.profile_scroll.current,
        );
        self.profile_scroll.current = profile_scroll;
        if scrollbar_interacted {
            self.profile_scroll.sync_target();
        }
    }

    fn draw_analyzer_card(&self, card: AnalyzerCardKind, rect: Rect) -> Rect {
        let mouse = mouse_vec();
        let active = matches!(self.active_analyzer_card, Some(interaction) if interaction.card == card);
        let header_rect = analyzer_card_header_rect(rect);
        let header_hovered = header_rect.contains(mouse);
        let tab_border_color = if active || header_hovered {
            BAR_FILL
        } else {
            PANEL_BORDER
        };
        let tab_background = if active {
            color_u8!(40, 66, 61, 255)
        } else if header_hovered {
            BUTTON_IDLE
        } else {
            BAR_BACKGROUND
        };
        let title_color = if active || header_hovered {
            TEXT_PRIMARY
        } else {
            TEXT_SECONDARY
        };

        draw_rectangle(rect.x, rect.y, rect.w, rect.h, PANEL);
        draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.5, PANEL_BORDER);

        draw_rectangle(header_rect.x, header_rect.y, header_rect.w, header_rect.h, tab_background);
        draw_rectangle_lines(header_rect.x, header_rect.y, header_rect.w, header_rect.h, 1.0, tab_border_color);
        draw_line(
            header_rect.x,
            header_rect.y + header_rect.h,
            header_rect.x + header_rect.w,
            header_rect.y + header_rect.h,
            1.0,
            tab_border_color,
        );
        draw_text_ex(
            analyzer_card_title(card),
            header_rect.x + 14.0,
            header_rect.y + 26.0,
            TextParams {
                font_size: 20,
                color: title_color,
                ..Default::default()
            },
        );

        let resize_rect = analyzer_card_resize_rect(rect);
        let grip_color = if active || resize_rect.contains(mouse) {
            TEXT_PRIMARY
        } else {
            TEXT_SECONDARY
        };
        draw_line(
            resize_rect.x + 4.0,
            resize_rect.y + resize_rect.h - 4.0,
            resize_rect.x + resize_rect.w - 4.0,
            resize_rect.y + 4.0,
            1.5,
            grip_color,
        );
        draw_line(
            resize_rect.x + 8.0,
            resize_rect.y + resize_rect.h - 4.0,
            resize_rect.x + resize_rect.w - 4.0,
            resize_rect.y + 8.0,
            1.5,
            grip_color,
        );

        Rect::new(
            rect.x + ANALYZER_CARD_PADDING,
            rect.y + ANALYZER_CARD_HEADER_HEIGHT + ANALYZER_CARD_PADDING,
            (rect.w - ANALYZER_CARD_PADDING * 2.0).max(0.0),
            (rect.h - ANALYZER_CARD_HEADER_HEIGHT - ANALYZER_CARD_PADDING * 2.0).max(0.0),
        )
    }

    fn draw_profile_card(&self, rect: Rect, content: &ProfileCardContent) {
        draw_rectangle(rect.x, rect.y, rect.w, rect.h, PANEL);
        draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, PANEL_BORDER);

        draw_text_ex(
            &content.player_name,
            rect.x + 14.0,
            rect.y + 26.0,
            TextParams {
                font_size: 20,
                color: TEXT_PRIMARY,
                ..Default::default()
            },
        );

        let mut cursor_y = draw_wrapped_text(
            &content.play_style,
            rect.x + 14.0,
            rect.y + 52.0,
            rect.w - 28.0,
            16,
            20.0,
            TEXT_SECONDARY,
        );
        cursor_y += 4.0;

        draw_text_ex(
            "Suggestions",
            rect.x + 14.0,
            cursor_y,
            TextParams {
                font_size: 16,
                color: TEXT_PRIMARY,
                ..Default::default()
            },
        );
        cursor_y += 18.0;

        for suggestion in &content.suggestions {
            cursor_y = draw_wrapped_text(
            &format!("- {suggestion}"),
                rect.x + 14.0,
                cursor_y,
                rect.w - 28.0,
                15,
                19.0,
                TEXT_SECONDARY,
            );
            cursor_y += 2.0;
        }
    }

    fn draw_manual_input_modal(&mut self) {
        draw_rectangle(0.0, 0.0, screen_width(), screen_height(), OVERLAY);

        let rect = Rect::new(screen_width() / 2.0 - 220.0, screen_height() / 2.0 - 180.0, 440.0, 360.0);
        draw_rectangle(rect.x, rect.y, rect.w, rect.h, PANEL);
        draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.5, PANEL_BORDER);

        draw_text_ex(
            "Manual Stat Input",
            rect.x + 18.0,
            rect.y + 34.0,
            TextParams {
                font_size: 28,
                color: TEXT_PRIMARY,
                ..Default::default()
            },
        );

        let current_state = self.manual_input.clone().unwrap_or_else(ManualInputState::new);
        let fields = [
            ("PPS", current_state.pps, TextField::ManualPps, "2.10"),
            ("APM", current_state.apm, TextField::ManualApm, "120.00"),
            ("VS Score", current_state.vs_score, TextField::ManualVsScore, "300.00"),
        ];

        let mut cursor_y = rect.y + 72.0;
        for (label, value, field, placeholder) in fields {
            draw_text_ex(
                label,
                rect.x + 18.0,
                cursor_y,
                TextParams {
                    font_size: 18,
                    color: TEXT_PRIMARY,
                    ..Default::default()
                },
            );

            let input_rect = Rect::new(rect.x + 18.0, cursor_y + 10.0, rect.w - 36.0, 40.0);
            if self.text_input_box(
                input_rect,
                &value,
                placeholder,
                self.active_text_field == Some(field),
                true,
                true,
            ) {
                self.active_text_field = Some(field);
            }

            cursor_y += 72.0;
        }

        if let Some(message) = self.manual_input.as_ref().and_then(|state| state.error_message.as_ref()) {
            draw_wrapped_text(
                message,
                rect.x + 18.0,
                rect.y + 292.0,
                rect.w - 36.0,
                16,
                20.0,
                color_u8!(255, 166, 166, 255),
            );
        }

        if self.overlay_button(
            Rect::new(rect.x + 18.0, rect.y + rect.h - 54.0, 120.0, 36.0),
            "Submit",
            true,
        ) {
            self.submit_manual_input();
        }

        if self.overlay_button(
            Rect::new(rect.x + rect.w - 138.0, rect.y + rect.h - 54.0, 120.0, 36.0),
            "Cancel",
            true,
        ) {
            self.close_manual_input();
        }
    }

    fn draw_progress_overlay(&mut self) {
        let task = match self.progress_task.as_ref() {
            Some(task) => task,
            None => return,
        };

        let total = task.total_files.max(1);
        let progress = task.completed_files.min(total) as f32 / total as f32;
        let current_file = if task.completed_files >= task.total_files {
            "Finalizing...".to_owned()
        } else {
            task.current_file.clone()
        };
        let label = task.label.clone();
        let completed = task.completed_files;

        draw_rectangle(0.0, 0.0, screen_width(), screen_height(), OVERLAY);

        let rect = Rect::new(screen_width() / 2.0 - 240.0, screen_height() / 2.0 - 100.0, 480.0, 200.0);
        draw_rectangle(rect.x, rect.y, rect.w, rect.h, PANEL);
        draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.5, PANEL_BORDER);

        draw_text_ex(
            &label,
            rect.x + 18.0,
            rect.y + 34.0,
            TextParams {
                font_size: 26,
                color: TEXT_PRIMARY,
                ..Default::default()
            },
        );
        draw_text_ex(
            &format!("{completed} / {} processed", task.total_files),
            rect.x + 18.0,
            rect.y + 64.0,
            TextParams {
                font_size: 18,
                color: TEXT_SECONDARY,
                ..Default::default()
            },
        );

        draw_wrapped_text(
            &current_file,
            rect.x + 18.0,
            rect.y + 92.0,
            rect.w - 36.0,
            18,
            22.0,
            TEXT_SECONDARY,
        );

        let bar_rect = Rect::new(rect.x + 18.0, rect.y + 118.0, rect.w - 36.0, 18.0);
        draw_rectangle(bar_rect.x, bar_rect.y, bar_rect.w, bar_rect.h, BAR_BACKGROUND);
        draw_rectangle(bar_rect.x, bar_rect.y, bar_rect.w * progress, bar_rect.h, BAR_FILL);

        if self.overlay_button(
            Rect::new(rect.x + rect.w - 132.0, rect.y + rect.h - 52.0, 114.0, 34.0),
            "Cancel",
            true,
        ) {
            self.cancel_progress_task();
        }
    }

    fn draw_panel(&self, rect: Rect, title: &str) {
        draw_rectangle(rect.x, rect.y, rect.w, rect.h, PANEL);
        draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.5, PANEL_BORDER);
        draw_text_ex(
            title,
            rect.x + 18.0,
            rect.y + 32.0,
            TextParams {
                font_size: 28,
                color: TEXT_PRIMARY,
                ..Default::default()
            },
        );
    }

    fn button(&self, rect: Rect, label: &str, enabled: bool) -> bool {
        self.button_inner(rect, label, enabled, false)
    }

    fn overlay_button(&self, rect: Rect, label: &str, enabled: bool) -> bool {
        self.button_inner(rect, label, enabled, true)
    }

    fn button_inner(&self, rect: Rect, label: &str, enabled: bool, allow_when_blocked: bool) -> bool {
        let interactive = enabled && (allow_when_blocked || !self.input_blocked());
        let hovered = rect.contains(mouse_vec());
        let color = if !interactive {
            BUTTON_DISABLED
        } else if hovered {
            BUTTON_HOVER
        } else {
            BUTTON_IDLE
        };

        draw_rectangle(rect.x, rect.y, rect.w, rect.h, color);
        draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, PANEL_BORDER);
        draw_text_centered(label, rect, 20, TEXT_PRIMARY);

        interactive && hovered && is_mouse_button_pressed(MouseButton::Left)
    }

    fn selectable_row(&self, rect: Rect, label: &str, selected: bool) -> bool {
        let interactive = !self.input_blocked();
        let hovered = interactive && rect.contains(mouse_vec());
        let row_color = if selected {
            SELECTED_ROW
        } else if hovered {
            BUTTON_IDLE
        } else {
            PANEL
        };

        draw_rectangle(rect.x, rect.y, rect.w, rect.h, row_color);
        draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, PANEL_BORDER);

        let indicator = if selected { "[x]" } else { "[ ]" };
        draw_text_ex(
            indicator,
            rect.x + 12.0,
            rect.y + rect.h / 2.0 + 6.0,
            TextParams {
                font_size: 18,
                color: TEXT_PRIMARY,
                ..Default::default()
            },
        );
        draw_text_ex(
            label,
            rect.x + 48.0,
            rect.y + rect.h / 2.0 + 6.0,
            TextParams {
                font_size: 18,
                color: TEXT_PRIMARY,
                ..Default::default()
            },
        );

        hovered && is_mouse_button_pressed(MouseButton::Left)
    }

    fn text_input_box(
        &self,
        rect: Rect,
        value: &str,
        placeholder: &str,
        focused: bool,
        enabled: bool,
        allow_when_blocked: bool,
    ) -> bool {
        let interactive = enabled && (allow_when_blocked || !self.input_blocked());
        let hovered = interactive && rect.contains(mouse_vec());
        let background = if focused {
            INPUT_FOCUSED
        } else {
            INPUT_BACKGROUND
        };
        let border = if hovered || focused { TEXT_PRIMARY } else { PANEL_BORDER };

        draw_rectangle(rect.x, rect.y, rect.w, rect.h, background);
        draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, border);

        let (text, color) = if value.is_empty() {
            (placeholder, TEXT_SECONDARY)
        } else {
            (value, TEXT_PRIMARY)
        };
        draw_text_ex(
            text,
            rect.x + 10.0,
            rect.y + rect.h / 2.0 + 6.0,
            TextParams {
                font_size: 18,
                color,
                ..Default::default()
            },
        );

        hovered && is_mouse_button_pressed(MouseButton::Left)
    }

    fn set_folder(&mut self, folder_path: PathBuf) {
        self.current_folder = Some(folder_path);
        self.clear_analysis();
        self.selected_files.clear();
        self.file_scroll.reset();
        self.profile_scroll.reset();
        self.refresh_files();
    }

    fn refresh_files(&mut self) {
        let Some(folder_path) = &self.current_folder else {
            return;
        };

        let read_dir = match fs::read_dir(folder_path) {
            Ok(read_dir) => read_dir,
            Err(error) => {
                self.replay_files.clear();
                self.status_message = format!("Failed to read folder: {error}");
                return;
            }
        };

        let mut replay_files: Vec<PathBuf> = read_dir
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("ttrm"))
                    .unwrap_or(false)
            })
            .collect();

        replay_files.sort_by_key(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_lowercase())
                .unwrap_or_default()
        });

        self.replay_files = replay_files;
        self.selected_files.clear();
        self.file_scroll.reset();
        self.profile_scroll.reset();
        if !matches!(self.current_source, Some(AnalysisSource::ManualInput)) {
            self.clear_analysis();
        }

        self.status_message = if self.replay_files.is_empty() {
            "No .ttrm files found in the selected folder.".to_owned()
        } else {
            format!(
                "Found {} replay file(s). Single-click to analyze or Ctrl+Click for batch selection.",
                self.replay_files.len()
            )
        };
    }

    fn open_manual_input(&mut self) {
        self.manual_input = Some(ManualInputState::new());
        self.active_text_field = Some(TextField::ManualPps);
    }

    fn close_manual_input(&mut self) {
        self.manual_input = None;
        if matches!(
            self.active_text_field,
            Some(TextField::ManualPps | TextField::ManualApm | TextField::ManualVsScore)
        ) {
            self.active_text_field = None;
        }
    }

    fn submit_manual_input(&mut self) {
        let Some(current_state) = self.manual_input.clone() else {
            return;
        };

        let parse_value = |value: &str, label: &str| -> Result<f32, String> {
            value
                .trim()
                .parse::<f32>()
                .map_err(|_| format!("{label} must be a valid number."))
        };

        let pps = match parse_value(&current_state.pps, "PPS") {
            Ok(value) => value,
            Err(error) => {
                if let Some(state) = self.manual_input.as_mut() {
                    state.error_message = Some(error);
                }
                return;
            }
        };
        let apm = match parse_value(&current_state.apm, "APM") {
            Ok(value) => value,
            Err(error) => {
                if let Some(state) = self.manual_input.as_mut() {
                    state.error_message = Some(error);
                }
                return;
            }
        };
        let vs_score = match parse_value(&current_state.vs_score, "VS Score") {
            Ok(value) => value,
            Err(error) => {
                if let Some(state) = self.manual_input.as_mut() {
                    state.error_message = Some(error);
                }
                return;
            }
        };

        let mut overall_stats = PlayerStats::new();
        overall_stats.insert("Manual Input".to_owned(), DerivedStats::from_base(pps, apm, vs_score));

        self.close_manual_input();
        self.selected_files.clear();
        self.apply_analysis(
            AnalysisResult {
                round_stats: Vec::new(),
                overall_stats,
                winner: None,
            },
            AnalysisSource::ManualInput,
            "Loaded manual stats into the analyzer.".to_owned(),
        );
    }

    fn start_analysis_task(&mut self, selected_indices: Vec<usize>) {
        if selected_indices.is_empty() || self.progress_task.is_some() {
            return;
        }

        let file_paths: Vec<PathBuf> = selected_indices
            .iter()
            .filter_map(|index| self.replay_files.get(*index).cloned())
            .collect();

        if file_paths.is_empty() {
            return;
        }

        let label = if file_paths.len() == 1 {
            "Analyzing replay".to_owned()
        } else {
            format!("Analyzing {} selected replays", file_paths.len())
        };

        self.start_progress_task(
            ProgressTaskMode::AnalyzeSelection { selected_indices },
            file_paths,
            false,
            label,
            None,
        );
    }

    fn start_cache_rebuild_task(&mut self) {
        if self.replay_files.is_empty() || self.progress_task.is_some() {
            return;
        }

        self.start_progress_task(
            ProgressTaskMode::RebuildCache,
            self.replay_files.clone(),
            true,
            "Rebuilding replay cache".to_owned(),
            self.current_source_paths(),
        );
    }

    fn start_progress_task(
        &mut self,
        mode: ProgressTaskMode,
        file_paths: Vec<PathBuf>,
        force_reprocess: bool,
        label: String,
        retained_paths: Option<BTreeSet<PathBuf>>,
    ) {
        if file_paths.is_empty() || self.progress_task.is_some() {
            return;
        }

        if let Err(error) = cache::ensure_cache_dir(&self.cache_dir) {
            self.status_message = format!("Failed to create cache directory: {error}");
            return;
        }

        let current_file = file_paths
            .first()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("Preparing...")
            .to_owned();
        let total_files = file_paths.len();
        let jobs = file_paths
            .into_iter()
            .map(|path| ReplayJob {
                file_name: path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Replay")
                    .to_owned(),
                path,
                force_reprocess,
            })
            .collect();

        self.progress_task = Some(ProgressTask {
            mode,
            label: label.clone(),
            total_files,
            completed_files: 0,
            current_file,
            successful_files: 0,
            successes: Vec::new(),
            failed_files: Vec::new(),
            retained_paths,
            background: spawn_background_processor(jobs, self.cache_dir.clone()),
        });
        self.status_message = format!("{label}...");
    }

    fn tick_progress_task(&mut self) {
        let Some(task) = self.progress_task.as_mut() else {
            return;
        };

        let mut should_finish = false;

        loop {
            match task.background.event_rx.try_recv() {
                Ok(WorkerEvent::FileDone(result)) => {
                    task.completed_files += 1;
                    task.current_file = result.file_name.clone();

                    match result.outcome {
                        Ok(processed) if processed.analysis.is_empty() => {
                            task.failed_files.push(result.file_name);
                        }
                        Ok(processed) => {
                            task.successful_files += 1;

                            let keep_result = task
                                .retained_paths
                                .as_ref()
                                .map_or(true, |paths| paths.contains(&result.path));
                            if keep_result {
                                task.successes.push((result.path, processed.analysis));
                            }
                        }
                        Err(_) => task.failed_files.push(result.file_name),
                    }
                }
                Ok(WorkerEvent::Finished) => {
                    should_finish = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    should_finish = true;
                    break;
                }
            }
        }

        if should_finish {
            self.finish_progress_task();
        }
    }

    fn finish_progress_task(&mut self) {
        let Some(mut task) = self.progress_task.take() else {
            return;
        };

        if let Some(handle) = task.background.completion_thread.take() {
            let _ = handle.join();
        }

        match task.mode {
            ProgressTaskMode::AnalyzeSelection { selected_indices } => {
                if task.successes.is_empty() {
                    self.clear_analysis();
                    self.status_message = if task.failed_files.is_empty() {
                        "Selected analysis completed with no results.".to_owned()
                    } else {
                        format!(
                            "Failed to process the selected replay files: {}",
                            summarize_failures(&task.failed_files)
                        )
                    };
                    return;
                }

                let analysis = if selected_indices.len() == 1 && task.successes.len() == 1 {
                    task.successes[0].1.clone()
                } else {
                    combine_analysis_results(task.successes.iter().map(|(_, result)| result))
                };
                let source = if selected_indices.len() == 1 {
                    AnalysisSource::SingleFile(selected_indices[0])
                } else {
                    AnalysisSource::MultiFile(selected_indices.clone())
                };

                let mut status_message = if selected_indices.len() == 1 {
                    let name = self
                        .replay_files
                        .get(selected_indices[0])
                        .and_then(|path| path.file_name())
                        .and_then(|name| name.to_str())
                        .unwrap_or("Replay");
                    format!("Loaded {name} from replay data or cache.")
                } else {
                    format!(
                        "Combined {} successful replay(s) from a {}-file selection.",
                        task.successes.len(),
                        selected_indices.len()
                    )
                };

                if !task.failed_files.is_empty() {
                    status_message.push_str(" Failed: ");
                    status_message.push_str(&summarize_failures(&task.failed_files));
                }

                self.apply_analysis(analysis, source, status_message);
            }
            ProgressTaskMode::RebuildCache => {
                if let Some(source) = self.current_source.clone() {
                    self.refresh_current_analysis_from_successes(&source, &task.successes);
                }

                self.status_message = if task.failed_files.is_empty() {
                    format!("Replay cache rebuilt for {} file(s).", task.successful_files)
                } else {
                    format!(
                        "Replay cache rebuilt with {} failure(s): {}",
                        task.failed_files.len(),
                        summarize_failures(&task.failed_files)
                    )
                };
            }
        }
    }

    fn cancel_progress_task(&mut self) {
        if let Some(task) = self.progress_task.take() {
            task.background.cancel_flag.store(true, Ordering::Relaxed);
            self.status_message = format!(
                "Canceled {} after {} of {} file(s).",
                task.label.to_lowercase(),
                task.completed_files,
                task.total_files
            );
        }
    }

    fn current_source_paths(&self) -> Option<BTreeSet<PathBuf>> {
        match self.current_source.as_ref()? {
            AnalysisSource::SingleFile(index) => self
                .replay_files
                .get(*index)
                .cloned()
                .map(|path| BTreeSet::from([path])),
            AnalysisSource::MultiFile(indices) => {
                let paths: BTreeSet<PathBuf> = indices
                    .iter()
                    .filter_map(|index| self.replay_files.get(*index).cloned())
                    .collect();
                if paths.is_empty() {
                    None
                } else {
                    Some(paths)
                }
            }
            AnalysisSource::ManualInput => None,
        }
    }

    fn refresh_current_analysis_from_successes(
        &mut self,
        source: &AnalysisSource,
        successes: &[(PathBuf, AnalysisResult)],
    ) {
        match source {
            AnalysisSource::SingleFile(index) => {
                let Some(path) = self.replay_files.get(*index) else {
                    return;
                };
                if let Some((_, result)) = successes.iter().find(|(candidate, _)| candidate == path) {
                    self.current_analysis = Some(result.clone());
                    self.current_round_index = result.round_stats.len();
                }
            }
            AnalysisSource::MultiFile(indices) => {
                let relevant: Vec<&AnalysisResult> = indices
                    .iter()
                    .filter_map(|index| self.replay_files.get(*index))
                    .filter_map(|path| successes.iter().find(|(candidate, _)| candidate == path))
                    .map(|(_, result)| result)
                    .collect();
                if relevant.is_empty() {
                    return;
                }
                self.current_analysis = Some(combine_analysis_results(relevant));
                self.current_round_index = 0;
            }
            AnalysisSource::ManualInput => {}
        }
    }

    fn apply_analysis(&mut self, analysis: AnalysisResult, source: AnalysisSource, status_message: String) {
        self.current_round_index = analysis.round_stats.len();
        self.current_analysis = Some(analysis);
        self.current_source = Some(source);
        self.status_message = status_message;
        self.profile_scroll.reset();
    }

    fn clear_analysis(&mut self) {
        self.current_analysis = None;
        self.current_source = None;
        self.current_round_index = 0;
        self.profile_scroll.reset();
    }

    fn current_display_state(&self) -> Option<DisplayState> {
        let analysis = self.current_analysis.as_ref()?;

        let (base_stats, base_winner, round_label) = if !analysis.round_stats.is_empty()
            && self.current_round_index < analysis.round_stats.len()
        {
            let stats = &analysis.round_stats[self.current_round_index];
            let winner = stats
                .iter()
                .max_by(|(_, left), (_, right)| left.vs_score.total_cmp(&right.vs_score))
                .map(|(player_name, _)| player_name.clone());
            (
                stats,
                winner,
                format!("Round {}", self.current_round_index + 1),
            )
        } else {
            (&analysis.overall_stats, analysis.winner.clone(), "Average".to_owned())
        };

        let filtered_stats = self.filter_stats(base_stats);
        let winner = base_winner.filter(|player_name| filtered_stats.contains_key(player_name));

        Some(DisplayState {
            stats: filtered_stats,
            winner,
            round_label,
        })
    }

    fn filtered_overall_stats(&self) -> PlayerStats {
        self.current_analysis
            .as_ref()
            .map(|analysis| self.filter_stats(&analysis.overall_stats))
            .unwrap_or_default()
    }

    fn analyzer_card_actual_rect(&self, card: AnalyzerCardKind, workspace: Rect) -> Rect {
        let normalized = self.analyzer_card_rect(card);
        Rect::new(
            workspace.x + normalized.x * workspace.w,
            workspace.y + normalized.y * workspace.h,
            normalized.w * workspace.w,
            normalized.h * workspace.h,
        )
    }

    fn analyzer_card_rect(&self, card: AnalyzerCardKind) -> Rect {
        match card {
            AnalyzerCardKind::WinnerTable => self.winner_table_card,
            AnalyzerCardKind::RadarCharts => self.radar_charts_card,
            AnalyzerCardKind::PlayerProfiles => self.player_profiles_card,
        }
    }

    fn analyzer_card_rect_mut(&mut self, card: AnalyzerCardKind) -> &mut Rect {
        match card {
            AnalyzerCardKind::WinnerTable => &mut self.winner_table_card,
            AnalyzerCardKind::RadarCharts => &mut self.radar_charts_card,
            AnalyzerCardKind::PlayerProfiles => &mut self.player_profiles_card,
        }
    }

    fn bring_analyzer_card_to_front(&mut self, card: AnalyzerCardKind) {
        if let Some(index) = self.analyzer_card_order.iter().position(|existing| *existing == card) {
            let mut reordered = self.analyzer_card_order;
            for slot in index..reordered.len().saturating_sub(1) {
                reordered[slot] = reordered[slot + 1];
            }
            reordered[reordered.len() - 1] = card;
            self.analyzer_card_order = reordered;
        }
    }

    fn update_analyzer_card_interaction(&mut self, workspace: Rect) {
        if workspace.w <= 0.0 || workspace.h <= 0.0 || self.input_blocked() {
            return;
        }

        let mouse = mouse_vec();

        if is_mouse_button_pressed(MouseButton::Left) {
            for card in self.analyzer_card_order.iter().rev().copied() {
                let rect = self.analyzer_card_actual_rect(card, workspace);
                if analyzer_card_resize_rect(rect).contains(mouse) {
                    self.bring_analyzer_card_to_front(card);
                    let front_rect = self.analyzer_card_actual_rect(card, workspace);
                    self.active_analyzer_card = Some(ActiveAnalyzerCardInteraction {
                        card,
                        mode: AnalyzerCardInteractionMode::Resize {
                            right_offset: front_rect.x + front_rect.w - mouse.x,
                            bottom_offset: front_rect.y + front_rect.h - mouse.y,
                        },
                    });
                    break;
                }

                if analyzer_card_header_rect(rect).contains(mouse) {
                    self.bring_analyzer_card_to_front(card);
                    let front_rect = self.analyzer_card_actual_rect(card, workspace);
                    self.active_analyzer_card = Some(ActiveAnalyzerCardInteraction {
                        card,
                        mode: AnalyzerCardInteractionMode::Drag {
                            grab_offset: mouse - vec2(front_rect.x, front_rect.y),
                        },
                    });
                    break;
                }

                if rect.contains(mouse) {
                    self.bring_analyzer_card_to_front(card);
                    break;
                }
            }
        }

        let Some(interaction) = self.active_analyzer_card else {
            return;
        };

        if !is_mouse_button_down(MouseButton::Left) {
            return;
        }

        let current_rect = self.analyzer_card_actual_rect(interaction.card, workspace);
        let updated_rect = match interaction.mode {
            AnalyzerCardInteractionMode::Drag { grab_offset } => Rect::new(
                mouse.x - grab_offset.x,
                mouse.y - grab_offset.y,
                current_rect.w,
                current_rect.h,
            ),
            AnalyzerCardInteractionMode::Resize {
                right_offset,
                bottom_offset,
            } => Rect::new(
                current_rect.x,
                current_rect.y,
                mouse.x + right_offset - current_rect.x,
                mouse.y + bottom_offset - current_rect.y,
            ),
        };

        let clamped_rect = self.clamp_analyzer_card_rect(interaction.card, workspace, updated_rect);
        self.set_analyzer_card_actual_rect(interaction.card, workspace, clamped_rect);
    }

    fn clamp_analyzer_card_rect(&self, card: AnalyzerCardKind, workspace: Rect, rect: Rect) -> Rect {
        let min_size = analyzer_card_min_size(card);
        let width = rect.w.max(min_size.x).min(workspace.w.max(0.0));
        let height = rect.h.max(min_size.y).min(workspace.h.max(0.0));
        let max_x = (workspace.x + workspace.w - width).max(workspace.x);
        let max_y = (workspace.y + workspace.h - height).max(workspace.y);

        Rect::new(
            rect.x.clamp(workspace.x, max_x),
            rect.y.clamp(workspace.y, max_y),
            width,
            height,
        )
    }

    fn set_analyzer_card_actual_rect(&mut self, card: AnalyzerCardKind, workspace: Rect, rect: Rect) {
        if workspace.w <= 0.0 || workspace.h <= 0.0 {
            return;
        }

        *self.analyzer_card_rect_mut(card) = Rect::new(
            ((rect.x - workspace.x) / workspace.w).clamp(0.0, 1.0),
            ((rect.y - workspace.y) / workspace.h).clamp(0.0, 1.0),
            (rect.w / workspace.w).clamp(0.0, 1.0),
            (rect.h / workspace.h).clamp(0.0, 1.0),
        );
    }

    fn filter_stats(&self, stats: &PlayerStats) -> PlayerStats {
        let filter_text = self.player_filter.trim().to_lowercase();
        if filter_text.is_empty() {
            return stats.clone();
        }

        stats
            .iter()
            .filter(|(player_name, _)| player_name.to_lowercase().contains(&filter_text))
            .map(|(player_name, player_stats)| (player_name.clone(), player_stats.clone()))
            .collect()
    }

    fn analysis_title(&self) -> String {
        match self.current_source.as_ref() {
            Some(AnalysisSource::SingleFile(index)) => self
                .replay_files
                .get(*index)
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str())
                .unwrap_or("Selected Replay")
                .to_owned(),
            Some(AnalysisSource::MultiFile(indices)) => {
                format!("Combined Analysis ({})", indices.len())
            }
            Some(AnalysisSource::ManualInput) => "Manual Input".to_owned(),
            None => "No Analysis Yet".to_owned(),
        }
    }

    fn analysis_summary(&self) -> String {
        if let Some(display) = self.current_display_state() {
            let total_players = self
                .current_analysis
                .as_ref()
                .map(|analysis| analysis.overall_stats.len())
                .unwrap_or(0);
            let winner = display.winner.as_deref().unwrap_or("Unknown");
            format!(
                "View: {}   |   Winner: {}   |   Visible Players: {} / {}",
                display.round_label,
                winner,
                display.stats.len(),
                total_players
            )
        } else {
            "Pick a replay, combine a multi-selection, or type a manual stat line to populate the analyzer.".to_owned()
        }
    }

    fn current_round_label(&self) -> String {
        self.current_display_state()
            .map(|display| display.round_label)
            .unwrap_or_else(|| "Average".to_owned())
    }

    fn selected_indices(&self) -> Vec<usize> {
        self.selected_files.iter().copied().collect()
    }

    fn input_blocked(&self) -> bool {
        self.manual_input.is_some() || self.progress_task.is_some()
    }

    fn draw_scrollbar(
        &mut self,
        target: ScrollbarTarget,
        rect: Rect,
        content_height: f32,
        viewport_height: f32,
        scroll_offset: f32,
    ) -> (f32, bool) {
        let metrics = scrollbar_metrics(rect, content_height, viewport_height, scroll_offset);
        let mouse = mouse_vec();
        let track_hovered = metrics.track_rect.contains(mouse);
        let thumb_hovered = metrics.thumb_rect.contains(mouse);
        let is_active = matches!(
            self.active_scrollbar_drag,
            Some(ActiveScrollbarDrag { target: active_target, .. }) if active_target == target
        );

        let border_color = if track_hovered || is_active {
            TEXT_PRIMARY
        } else {
            PANEL_BORDER
        };
        let thumb_color = if is_active {
            TEXT_PRIMARY
        } else if thumb_hovered {
            BAR_FILL
        } else {
            BUTTON_HOVER
        };

        draw_rectangle(rect.x, rect.y, rect.w, rect.h, INPUT_BACKGROUND);
        draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, border_color);
        draw_rectangle(
            metrics.thumb_rect.x,
            metrics.thumb_rect.y,
            metrics.thumb_rect.w,
            metrics.thumb_rect.h,
            thumb_color,
        );

        if self.input_blocked() {
            return (scroll_offset.clamp(0.0, metrics.max_scroll), false);
        }

        let mut new_scroll = scroll_offset.clamp(0.0, metrics.max_scroll);
        let mut interacted = false;

        if is_mouse_button_pressed(MouseButton::Left) && track_hovered {
            let grab_offset = if thumb_hovered {
                mouse.y - metrics.thumb_rect.y
            } else {
                metrics.thumb_rect.h / 2.0
            };
            self.active_scrollbar_drag = Some(ActiveScrollbarDrag { target, grab_offset });
            new_scroll = scrollbar_scroll_from_mouse(metrics, mouse.y, grab_offset);
            interacted = true;
        } else if is_mouse_button_down(MouseButton::Left) && is_active {
            let grab_offset = self
                .active_scrollbar_drag
                .map(|drag| drag.grab_offset)
                .unwrap_or(metrics.thumb_rect.h / 2.0);
            new_scroll = scrollbar_scroll_from_mouse(metrics, mouse.y, grab_offset);
            interacted = true;
        }

        (new_scroll.clamp(0.0, metrics.max_scroll), interacted)
    }

    fn handle_keyboard_input(&mut self) {
        if self.progress_task.is_some() && is_key_pressed(KeyCode::Escape) {
            self.cancel_progress_task();
            return;
        }

        if self.manual_input.is_some() && is_key_pressed(KeyCode::Escape) {
            self.close_manual_input();
            return;
        }

        let Some(field) = self.active_text_field else {
            return;
        };

        if !self.is_valid_focus(field) {
            self.active_text_field = None;
            return;
        }

        let mut field_changed = false;

        while let Some(character) = get_char_pressed() {
            if self.accept_character(field, character) {
                if let Some(target) = self.text_field_mut(field) {
                    target.push(character);
                    field_changed = true;
                }
            }
        }

        if is_key_pressed(KeyCode::Backspace) {
            if let Some(target) = self.text_field_mut(field) {
                target.pop();
                field_changed = true;
            }
        }

        if field == TextField::PlayerFilter && field_changed {
            self.profile_scroll.reset();
        }

        if self.manual_input.is_some() {
            if is_key_pressed(KeyCode::Tab) {
                self.cycle_manual_focus();
            }

            if is_key_pressed(KeyCode::Enter) {
                self.submit_manual_input();
            }
        }
    }

    fn is_valid_focus(&self, field: TextField) -> bool {
        match field {
            TextField::PlayerFilter => true,
            TextField::ManualPps | TextField::ManualApm | TextField::ManualVsScore => {
                self.manual_input.is_some()
            }
        }
    }

    fn accept_character(&self, field: TextField, character: char) -> bool {
        match field {
            TextField::PlayerFilter => !character.is_control(),
            TextField::ManualPps | TextField::ManualApm | TextField::ManualVsScore => {
                character.is_ascii_digit() || character == '.'
            }
        }
    }

    fn text_field_mut(&mut self, field: TextField) -> Option<&mut String> {
        match field {
            TextField::PlayerFilter => Some(&mut self.player_filter),
            TextField::ManualPps => self.manual_input.as_mut().map(|state| &mut state.pps),
            TextField::ManualApm => self.manual_input.as_mut().map(|state| &mut state.apm),
            TextField::ManualVsScore => self.manual_input.as_mut().map(|state| &mut state.vs_score),
        }
    }

    fn cycle_manual_focus(&mut self) {
        self.active_text_field = Some(match self.active_text_field {
            Some(TextField::ManualPps) => TextField::ManualApm,
            Some(TextField::ManualApm) => TextField::ManualVsScore,
            _ => TextField::ManualPps,
        });
    }
}

pub fn install_ui_font(font: Font) {
    let mut stored_font = ui_font_store().lock().unwrap();
    *stored_font = Some(font);
}

fn radar_points<F>(center: Vec2, radius: f32, count: usize, mut value_for_index: F) -> Vec<Vec2>
where
    F: FnMut(usize) -> f32,
{
    (0..count)
        .map(|index| {
            let angle = -PI / 2.0 + TAU * index as f32 / count as f32;
            let scaled_radius = radius * value_for_index(index);
            vec2(
                center.x + scaled_radius * angle.cos(),
                center.y + scaled_radius * angle.sin(),
            )
        })
        .collect()
}

fn draw_polyline(points: &[Vec2], color: Color, thickness: f32) {
    if points.len() < 2 {
        return;
    }

    for index in 0..points.len() {
        let current = points[index];
        let next = points[(index + 1) % points.len()];
        draw_line(current.x, current.y, next.x, next.y, thickness, color);
    }
}

fn distinct_player_colors(count: usize) -> Vec<Color> {
    (0..count)
        .map(|index| hsv_color(index as f32 / count.max(1) as f32, 0.65, 0.92))
        .collect()
}

fn hsv_color(hue: f32, saturation: f32, value: f32) -> Color {
    let hue = hue.rem_euclid(1.0) * 6.0;
    let chroma = value * saturation;
    let x = chroma * (1.0 - ((hue % 2.0) - 1.0).abs());

    let (red, green, blue) = match hue as i32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };

    let m = value - chroma;
    Color::new(red + m, green + m, blue + m, 1.0)
}

fn build_profile_card_content(player_name: &str, player_stats: &DerivedStats, card_width: f32) -> ProfileCardContent {
    let mut profile = PlayerProfile::new(player_name.to_owned());
    profile.add_game(player_stats);

    let play_style = analyze_play_style(&profile);
    let suggestions = improvement_suggestions(&profile)
        .into_iter()
        .take(3)
        .collect::<Vec<_>>();
    let text_width = (card_width - 28.0).max(80.0);

    let mut height = 52.0;
    height += measure_wrapped_text_height(&play_style, text_width, 16, 20.0);
    height += 4.0;
    height += 18.0;
    height += 18.0;
    for suggestion in &suggestions {
        height += measure_wrapped_text_height(&format!("- {suggestion}"), text_width, 15, 19.0);
        height += 2.0;
    }
    height += 12.0;

    ProfileCardContent {
        player_name: profile.username,
        play_style,
        suggestions,
        height,
    }
}

fn scrollbar_metrics(rect: Rect, content_height: f32, viewport_height: f32, scroll_offset: f32) -> ScrollbarMetrics {
    let track_rect = Rect::new(
        rect.x + 1.0,
        rect.y + 1.0,
        (rect.w - 2.0).max(0.0),
        (rect.h - 2.0).max(0.0),
    );
    let content_height = content_height.max(viewport_height).max(1.0);
    let viewport_height = viewport_height.max(0.0);
    let max_scroll = (content_height - viewport_height).max(0.0);
    let min_thumb_height = track_rect.h.min(28.0);
    let thumb_height = if max_scroll <= f32::EPSILON {
        track_rect.h
    } else {
        let proportional_height = viewport_height / content_height * track_rect.h;
        proportional_height.max(min_thumb_height).min(track_rect.h)
    };
    let thumb_range = (track_rect.h - thumb_height).max(0.0);
    let progress = if max_scroll <= f32::EPSILON {
        0.0
    } else {
        (scroll_offset / max_scroll).clamp(0.0, 1.0)
    };
    let thumb_y = track_rect.y + thumb_range * progress;

    ScrollbarMetrics {
        track_rect,
        thumb_rect: Rect::new(track_rect.x, thumb_y, track_rect.w, thumb_height),
        max_scroll,
        thumb_range,
    }
}

fn scrollbar_scroll_from_mouse(metrics: ScrollbarMetrics, mouse_y: f32, grab_offset: f32) -> f32 {
    if metrics.max_scroll <= f32::EPSILON || metrics.thumb_range <= f32::EPSILON {
        return 0.0;
    }

    let thumb_top = (mouse_y - grab_offset).clamp(metrics.track_rect.y, metrics.track_rect.y + metrics.thumb_range);
    let progress = (thumb_top - metrics.track_rect.y) / metrics.thumb_range;
    progress * metrics.max_scroll
}

fn normalized_wheel_delta(wheel_y: f32) -> f32 {
    wheel_y.clamp(-MAX_WHEEL_SCROLL_STEPS, MAX_WHEEL_SCROLL_STEPS)
}

fn update_scroll_state(
    scroll: &mut ScrollState,
    viewport: Rect,
    content_height: f32,
    step: f32,
    input_blocked: bool,
) {
    let max_scroll = (content_height - viewport.h).max(0.0);
    scroll.target = scroll.target.clamp(0.0, max_scroll);
    scroll.current = scroll.current.clamp(0.0, max_scroll);

    if !input_blocked && viewport.contains(mouse_vec()) {
        let (_, wheel_y) = mouse_wheel();
        if wheel_y.abs() > f32::EPSILON {
            scroll.target = (scroll.target - normalized_wheel_delta(wheel_y) * step)
                .clamp(0.0, max_scroll);
        }
    }

    scroll.current = smooth_scroll_offset(scroll.current, scroll.target).clamp(0.0, max_scroll);
}

fn smooth_scroll_offset(current: f32, target: f32) -> f32 {
    let delta = target - current;
    if delta.abs() <= 0.5 {
        return target;
    }

    let blend = (get_frame_time() * 18.0).clamp(0.0, 1.0);
    current + delta * blend
}

fn with_scissor(rect: Rect, draw: impl FnOnce()) {
    unsafe {
        let mut gl = macroquad::window::get_internal_gl();
        gl.flush();
        gl.quad_gl.scissor(Some((
            rect.x.floor() as i32,
            rect.y.floor() as i32,
            rect.w.ceil() as i32,
            rect.h.ceil() as i32,
        )));
    }

    draw();

    unsafe {
        let mut gl = macroquad::window::get_internal_gl();
        gl.flush();
        gl.quad_gl.scissor(None);
    }
}

fn wrapped_lines(text: &str, max_width: f32, font_size: u16) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current_line = String::new();

    for word in text.split_whitespace() {
        let candidate = if current_line.is_empty() {
            word.to_owned()
        } else {
            format!("{current_line} {word}")
        };

        if measure_text(&candidate, None, font_size, 1.0).width > max_width && !current_line.is_empty() {
            lines.push(current_line);
            current_line = word.to_owned();
        } else {
            current_line = candidate;
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    lines
}

fn measure_wrapped_text_height(text: &str, max_width: f32, font_size: u16, line_height: f32) -> f32 {
    wrapped_lines(text, max_width, font_size).len() as f32 * line_height
}

fn summarize_failures(failed_files: &[String]) -> String {
    let mut summary = failed_files.iter().take(4).cloned().collect::<Vec<_>>().join(", ");
    if failed_files.len() > 4 {
        summary.push_str(", ...");
    }
    summary
}

fn mouse_vec() -> Vec2 {
    let (x, y) = mouse_position();
    vec2(x, y)
}

fn analyzer_card_title(card: AnalyzerCardKind) -> &'static str {
    match card {
        AnalyzerCardKind::WinnerTable => "Winner Table",
        AnalyzerCardKind::RadarCharts => "Radar Charts",
        AnalyzerCardKind::PlayerProfiles => "Player Profiles",
    }
}

fn analyzer_card_min_size(card: AnalyzerCardKind) -> Vec2 {
    match card {
        AnalyzerCardKind::WinnerTable => vec2(ANALYZER_CARD_MIN_TABLE_WIDTH, ANALYZER_CARD_MIN_TABLE_HEIGHT),
        AnalyzerCardKind::RadarCharts => vec2(ANALYZER_CARD_MIN_RADAR_WIDTH, ANALYZER_CARD_MIN_RADAR_HEIGHT),
        AnalyzerCardKind::PlayerProfiles => {
            vec2(ANALYZER_CARD_MIN_PROFILE_WIDTH, ANALYZER_CARD_MIN_PROFILE_HEIGHT)
        }
    }
}

fn analyzer_card_header_rect(rect: Rect) -> Rect {
    Rect::new(rect.x, rect.y, rect.w, ANALYZER_CARD_HEADER_HEIGHT.min(rect.h.max(0.0)))
}

fn analyzer_card_resize_rect(rect: Rect) -> Rect {
    let size = ANALYZER_CARD_RESIZE_HANDLE.min(rect.w.max(0.0)).min(rect.h.max(0.0));
    Rect::new(rect.x + rect.w - size, rect.y + rect.h - size, size, size)
}

fn ui_font_store() -> &'static Mutex<Option<Font>> {
    UI_FONT.get_or_init(|| Mutex::new(None))
}

fn with_ui_font<T>(callback: impl FnOnce(Option<&Font>) -> T) -> T {
    let stored_font = ui_font_store().lock().unwrap();
    callback(stored_font.as_ref())
}

fn measure_text(text: &str, font: Option<&Font>, font_size: u16, font_scale: f32) -> TextDimensions {
    if font.is_some() {
        return macroquad::prelude::measure_text(text, font, font_size, font_scale);
    }

    with_ui_font(|ui_font| macroquad::prelude::measure_text(text, ui_font, font_size, font_scale))
}

fn draw_text_ex(text: &str, x: f32, y: f32, params: TextParams) {
    if params.font.is_some() {
        macroquad::prelude::draw_text_ex(text, x, y, params);
        return;
    }

    with_ui_font(|ui_font| {
        let params = TextParams {
            font: ui_font,
            ..params
        };
        macroquad::prelude::draw_text_ex(text, x, y, params);
    });
}

fn draw_text_centered(text: &str, rect: Rect, font_size: u16, color: Color) {
    let size = measure_text(text, None, font_size, 1.0);
    draw_text_ex(
        text,
        rect.x + (rect.w - size.width) / 2.0,
        rect.y + rect.h / 2.0 + size.height / 2.5,
        TextParams {
            font_size,
            color,
            ..Default::default()
        },
    );
}

fn draw_wrapped_text(
    text: &str,
    x: f32,
    mut y: f32,
    max_width: f32,
    font_size: u16,
    line_height: f32,
    color: Color,
) -> f32 {
    for line in wrapped_lines(text, max_width, font_size) {
        draw_text_ex(
            &line,
            x,
            y,
            TextParams {
                font_size,
                color,
                ..Default::default()
            },
        );
        y += line_height;
    }

    y
}

fn spawn_background_processor(jobs: Vec<ReplayJob>, cache_dir: PathBuf) -> BackgroundProcessor {
    let worker_count = preferred_worker_count(jobs.len());
    let (job_tx, job_rx) = crossbeam_channel::bounded::<ReplayJob>((worker_count * 2).max(1));
    let (event_tx, event_rx) = crossbeam_channel::unbounded::<WorkerEvent>();
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let mut worker_handles = Vec::with_capacity(worker_count);

    for _ in 0..worker_count {
        let worker_rx = job_rx.clone();
        let worker_tx = event_tx.clone();
        let worker_cancel = Arc::clone(&cancel_flag);
        let worker_cache_dir = cache_dir.clone();
        worker_handles.push(thread::spawn(move || {
            worker_loop(worker_rx, worker_tx, worker_cancel, worker_cache_dir);
        }));
    }

    let dispatch_cancel = Arc::clone(&cancel_flag);
    let dispatcher = thread::spawn(move || {
        for job in jobs {
            if dispatch_cancel.load(Ordering::Relaxed) {
                break;
            }

            if job_tx.send(job).is_err() {
                break;
            }
        }
    });

    let completion_tx = event_tx.clone();
    let completion_thread = thread::spawn(move || {
        let _ = dispatcher.join();
        for handle in worker_handles {
            let _ = handle.join();
        }
        let _ = completion_tx.send(WorkerEvent::Finished);
    });

    BackgroundProcessor {
        event_rx,
        cancel_flag,
        completion_thread: Some(completion_thread),
    }
}

fn worker_loop(
    job_rx: Receiver<ReplayJob>,
    event_tx: Sender<WorkerEvent>,
    cancel_flag: Arc<AtomicBool>,
    cache_dir: PathBuf,
) {
    while let Ok(job) = job_rx.recv() {
        if cancel_flag.load(Ordering::Relaxed) {
            break;
        }

        let ReplayJob {
            path,
            file_name,
            force_reprocess,
        } = job;
        let prepared = cache::prepare_replay(&path, &cache_dir);
        let outcome = cache::process_prepared_replay(&prepared, force_reprocess)
            .map_err(|error| error.to_string());

        if cancel_flag.load(Ordering::Relaxed) {
            break;
        }

        if event_tx
            .send(WorkerEvent::FileDone(WorkerResult {
                path,
                file_name,
                outcome,
            }))
            .is_err()
        {
            break;
        }
    }
}

fn preferred_worker_count(total_files: usize) -> usize {
    let available = thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
        .clamp(2, 8);
    total_files.min(available).max(1)
}