#![no_std]

mod memory;
mod stages;

use asr::{
    future::{next_tick, retry},
    settings::{gui::Title, Gui},
    time::Duration,
    timer::{self, TimerState},
    watcher::{Pair, Watcher},
    Process,
};
use memory::{update_watchers, Memory};
use stages::GameStage;

asr::async_main!(stable);
asr::panic_handler!();

async fn main() {
    let mut settings = Settings::register();

    asr::print_message("PACMAN REPAC TWOOOOOOO autosplitter loaded");

    loop {
        let process = Process::wait_attach("PAC-MAN WORLD 2 Re-PAC.exe").await;
        process
            .until_closes(async {
                // INIT
                // Once the target has been found and attached to, set up some default watchers
                let mut watchers = Watchers::default();

                let mut enable_il_restart = false;
                let mut enable_level_split = false;
                let mut last_time_trial_split_time: f64 = 0.;
                let mut highest_boss_phase_split = 0;

                let mut time_trial_marathon_timer_acum: f64 = 0.;
                let mut restarting_level = false;

                // Perform memory scanning to look for the addresses we need
                let mut memory = retry(|| Memory::init(&process)).await;
                loop {
                    // MAIN LOOP
                    settings.update();
                    update_watchers(&process, &mut memory, &mut watchers, &settings);

                    // get memory values
                    let is_loading_pair = watchers.is_loading.pair.unwrap_or_default();
                    let time_trial_igt_pair = watchers.time_trial_igt.pair.unwrap_or_default();
                    let time_trial_state_pair =
                        watchers.time_trial_state.pair.unwrap_or(Pair::default());
                    let time_trial_bonus_pair = watchers
                        .time_trial_bonus_time
                        .pair
                        .unwrap_or(Pair::default());
                    let load_ui_progress_pair =
                        watchers.load_ui_progress.pair.unwrap_or(Pair::default());
                    let player_state_pair = watchers.player_state.pair.unwrap_or_default();

                    match settings.timer_mode.current {
                        TimerMode::FullGame => {
                            if is_loading_pair.current
                                || (load_ui_progress_pair.current > 0.0
                                    && load_ui_progress_pair.current < 1.0)
                            {
                                timer::pause_game_time();
                            } else {
                                timer::resume_game_time();
                            }

                            if start(&watchers, &settings) {
                                if settings.reset_on_file_creation {
                                    timer::reset();
                                }
                                timer::start();
                                // timing starts on difficulty select so we manually add the animation time before the loading starts
                                // if i manage to detect that from memory then this will be removed
                                timer::set_game_time(Duration::new(3, 433_333_333));
                            }

                            // only do level splits if player actually completed the level
                            // or it's from pac-village
                            if !enable_level_split {
                                enable_level_split = enable_full_game_level_splits(&watchers);
                            }

                            if split_full_game(&watchers, &settings, enable_level_split) {
                                timer::split();
                                enable_level_split = false;
                            }
                        }
                        TimerMode::IL => {
                            let stage_state_pair = watchers.stage_state.pair.unwrap_or_default();
                            let checkpoint_pair = watchers.checkpoint.pair.unwrap_or_default();
                            let boss_phase_pair = watchers.boss_state.pair.unwrap_or_default();

                            // 3 cases that enable timer start:
                            // * restart from menu while player is not dead and checkpoint is -1 (works on stage start before checkpoints)
                            // * checkpoint returns to -1 while stage state is "pac dead"
                            // * start from level select
                            // TODO fix submarine levels
                            if (stage_state_pair.old == StageState::Pause
                                && stage_state_pair.current == StageState::PacDead
                                && player_state_pair.current != PlayerState::Dead
                                && checkpoint_pair.current == -1)
                                || (checkpoint_pair.changed()
                                    && checkpoint_pair.current == -1
                                    && stage_state_pair.current == StageState::PacDead)
                                || (player_state_pair.old == PlayerState::StageInit
                                    && (player_state_pair.current == PlayerState::Control
                                        || player_state_pair.current == PlayerState::Shooting))
                            {
                                enable_il_restart = true;
                            }

                            if ((player_state_pair.old != PlayerState::Control
                                && player_state_pair.current == PlayerState::Control)
                                || player_state_pair.old != PlayerState::Shooting
                                    && player_state_pair.current == PlayerState::Shooting)
                                && enable_il_restart
                            {
                                if settings.reset_on_level_start {
                                    timer::reset();
                                    timer::resume_game_time();
                                }
                                if settings.start_il {
                                    if timer::state() != TimerState::Running {
                                        timer::start();
                                        timer::set_game_time(Duration::seconds(0));
                                    }
                                    highest_boss_phase_split = 0;
                                }
                                enable_il_restart = false;
                            }

                            if player_state_pair.current != player_state_pair.old
                                && player_state_pair.current == PlayerState::Goal
                                && settings.split_il
                            {
                                // JANK SOLUTION to finish the run even when there are splits pending from skipping checkpoints
                                for _ in 0..100 {
                                    timer::skip_split();
                                }
                                // end run :)
                                timer::split();
                            }

                            if split_checkpoints(&checkpoint_pair, &settings)
                                || split_boss_phase(
                                    &boss_phase_pair,
                                    &settings,
                                    &mut highest_boss_phase_split,
                                )
                            {
                                timer::split();
                            }
                        }
                        TimerMode::TimeTrial => {
                            timer::pause_game_time();
                            let checkpoint_pair = watchers.checkpoint.pair.unwrap_or_default();
                            let boss_phase_pair = watchers.boss_state.pair.unwrap_or_default();
                            let stage_pair = watchers.level_id.pair.unwrap_or_default();
                            let igt_with_bonus = time_trial_igt_pair.current
                                - (time_trial_bonus_pair.current as f64);

                            if settings.time_trial_discount_bonus {
                                timer::set_game_time(Duration::seconds_f64(igt_with_bonus));
                            } else {
                                timer::set_game_time(Duration::seconds_f64(
                                    time_trial_igt_pair.current,
                                ));
                            }

                            if time_trial_state_pair.old == TimeTrialState::None
                                && time_trial_state_pair.current == TimeTrialState::TA
                            {
                                timer::start();
                                last_time_trial_split_time = 0.;
                                highest_boss_phase_split = 0;
                            }

                            if time_trial_state_pair.old != TimeTrialState::End
                                && time_trial_state_pair.current == TimeTrialState::End
                            {
                                // JANK SOLUTION to finish the run even when there are splits pending from skipping checkpoints
                                for _ in 0..100 {
                                    timer::skip_split();
                                }
                                timer::split();
                            }

                            if split_checkpoints(&checkpoint_pair, &settings) {
                                // check if it should skip the split because of a negative split time
                                if settings.time_trial_skip_negative
                                    && settings.time_trial_discount_bonus
                                    && last_time_trial_split_time > igt_with_bonus
                                {
                                    timer::skip_split();
                                } else {
                                    timer::split();
                                    last_time_trial_split_time = igt_with_bonus;
                                }
                            }

                            if split_boss_phase(
                                &boss_phase_pair,
                                &settings,
                                &mut highest_boss_phase_split,
                            ) {
                                timer::split();
                            }

                            // reset on trial set to None or return to stage select
                            if (time_trial_state_pair.current == TimeTrialState::None
                                && time_trial_igt_pair.current != time_trial_igt_pair.old)
                                || (stage_pair.changed()
                                    && level_is_stage_select(stage_pair.current))
                            {
                                timer::reset();
                            }
                        }
                        TimerMode::TimeTrialMarathon => {
                            timer::pause_game_time();
                            let stage_pair = watchers.level_id.pair.unwrap_or_default();
                            let stage_state_pair = watchers.stage_state.pair.unwrap_or_default();
                            let boss_state = watchers.boss_state.pair.unwrap_or_default();

                            if time_trial_state_pair.current != time_trial_state_pair.old
                                && time_trial_state_pair.current == TimeTrialState::TA
                                && player_state_pair.current == PlayerState::Control
                                && timer::state() == TimerState::NotRunning
                            {
                                time_trial_marathon_timer_acum = 0.;
                                timer::start();
                            }

                            if restarting_level && player_state_pair.current == PlayerState::Control
                            {
                                restarting_level = false;
                            }

                            let current_igt_with_bonus = time_trial_igt_pair.current
                                - (time_trial_bonus_pair.current as f64);

                            // split + accum time on time trial ended normally or state 4 (dead) on certain bosses
                            let boss_special_case = stage_pair.current == GameStage::Stage6_4
                                || stage_pair.current == GameStage::Stage6_4Past
                                || stage_pair.current == GameStage::Stage6_5
                                || stage_pair.current == GameStage::StageSonic3;
                            let level_ended = (time_trial_state_pair.changed()
                                && time_trial_state_pair.current == TimeTrialState::End)
                                || (boss_special_case
                                    && boss_state.changed()
                                    && boss_state.current == 4);

                            // accum igt from previous levels/runs
                            if level_ended {
                                // backup the timer after finishing a level0
                                time_trial_marathon_timer_acum += current_igt_with_bonus;
                            }
                            // backup the time during the pause screen if the player restarts level manually, without bonus clocks
                            if (stage_state_pair.old == StageState::Pause
                                || stage_state_pair.old == StageState::DebugPause)
                                && stage_state_pair.current == StageState::PacDead
                            {
                                time_trial_marathon_timer_acum += time_trial_igt_pair.old;
                                restarting_level = true;
                            }

                            // set the igt
                            if !(restarting_level
                                || time_trial_state_pair.current != TimeTrialState::TA
                                    && time_trial_state_pair.current != TimeTrialState::Pause
                                || boss_special_case && boss_state.current == 4)
                            {
                                timer::set_game_time(Duration::seconds_f64(
                                    time_trial_marathon_timer_acum + current_igt_with_bonus,
                                ));
                            } else {
                                timer::set_game_time(Duration::seconds_f64(
                                    time_trial_marathon_timer_acum,
                                ));
                            }

                            if level_ended {
                                timer::split();
                            }

                            timer::set_variable_float(
                                "IGT Accumulated",
                                time_trial_marathon_timer_acum,
                            );
                        }
                    }

                    next_tick().await;
                }
            })
            .await;
    }
}

#[derive(Gui, Clone, Copy, PartialEq)]
pub enum TimerMode {
    /// Full Game
    #[default]
    FullGame,
    /// Individual Level
    IL,
    /// Time Trial
    TimeTrial,
    /// Time Trial Marathon
    TimeTrialMarathon,
}

#[derive(Gui)]
struct Settings {
    /// LiveSplit Timer Mode
    _timer_mode: Title,

    /// Pick a Mode
    timer_mode: Pair<TimerMode>,

    /// Start Options
    _title_start: Title,

    /// Full Game New File
    #[default = true]
    start_new_game: bool,

    /// Individual Level
    #[default = true]
    start_il: bool,

    /// Split Options
    _title_split: Title,

    /// Level Exit
    #[default = true]
    split_on_level_complete: bool,

    /// Level Exit (Past)
    #[default = true]
    split_on_past_level_complete: bool,

    /// Spooky Defeat
    #[default = true]
    split_spooky_qte: bool,

    /// Toc-Man Defeat
    #[default = true]
    split_tocman: bool,

    /// Individual Level End
    #[default = true]
    split_il: bool,

    /// Individual Level Boss Phase
    #[default = false]
    split_boss_phase: bool,

    /// Individual Level Checkpoints
    ///
    /// You will need the same number of splits before the final one and checkpoints.
    /// For example, "Butane Pain" has 8 checkpoints, so you will need 9 total splits for optimal use.
    #[default = false]
    split_checkpoint: bool,

    /// Reset Options
    _title_reset: Title,

    /// New File
    #[default = true]
    reset_on_file_creation: bool,

    /// Individual Level Start
    #[default = true]
    reset_on_level_start: bool,

    /// Misc
    _misc_title: Title,

    /// Discount Bonus Time on Time Trials
    #[default = true]
    time_trial_discount_bonus: bool,

    /// Skip Negative Split Times on Time Trials
    ///
    /// This way the delta column and sum of best will be more consistent
    #[default = true]
    time_trial_skip_negative: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum TimeTrialState {
    None,
    ReadyInit,
    ReadyWait,
    TA,
    Pause,
    End,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum PlayerState {
    #[default]
    None,
    Control,
    Damage,
    FallDamage,
    IcePoolDamage,
    SnowBallDamage,
    SinkDamage,
    CutIn,
    CutInGrap,
    Gimmick,
    SpaceJump,
    SpaceJumpOut,
    StageInit,
    StageInitMaze,
    StageInitSJ,
    Dead,
    Goal,
    StageEnd,
    Shooting,
    Racing,
    ASRReadError,
    ASROffsetNotReady,
    Unknown,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum StageState {
    #[default]
    None,
    InitOnFade,
    InitEndFade,
    Playing,
    Pause,
    DebugPause,
    Maze,
    PacDead,
    GameOver,
    Goal,
    Exit,
    Unknown,
}

#[derive(Default)]
struct Watchers {
    is_loading: Watcher<bool>,
    load_ui_progress: Watcher<f32>,
    level_id: Watcher<GameStage>,
    checkpoint: Watcher<i32>,
    time_trial_igt: Watcher<f64>,
    time_trial_state: Watcher<TimeTrialState>,
    time_trial_bonus_time: Watcher<u32>,
    spooky_qte_success: Watcher<bool>,
    boss_state: Watcher<u32>,
    player_state: Watcher<PlayerState>,
    stage_state: Watcher<StageState>,
}

fn start(watchers: &Watchers, settings: &Settings) -> bool {
    let level_pair = if let Some(pair) = &watchers.level_id.pair {
        pair
    } else {
        return false;
    };

    if !level_pair.changed() {
        return false;
    }

    level_pair.current == GameStage::Movie
        && level_pair.old == GameStage::Title
        && settings.start_new_game
}

fn split_full_game(watchers: &Watchers, settings: &Settings, level_split_enabled: bool) -> bool {
    // level exit split
    let level_pair = watchers.level_id.pair.unwrap_or_default();

    if level_pair.changed() && level_split_enabled {
        match level_pair.current {
            GameStage::StageSelect => return settings.split_on_level_complete,
            GameStage::StageSelectPast => return settings.split_on_past_level_complete,
            GameStage::StageSelectSonic => return settings.split_on_level_complete,
            _ => return false,
        }
    };

    // spooky qte final split
    let spooky_pair = watchers.spooky_qte_success.pair.unwrap_or_default();
    if spooky_pair.changed() && spooky_pair.current && settings.split_spooky_qte {
        return true;
    }

    // tocman defeat split
    let boss_state_pair = watchers.boss_state.pair.unwrap_or_default();
    boss_state_pair.changed()
        && boss_state_pair.current == 4
        && level_pair.current == GameStage::Stage6_5
        && settings.split_tocman
}

fn enable_full_game_level_splits(watchers: &Watchers) -> bool {
    let player_state_pair = watchers.player_state.pair.unwrap_or_default();
    let stage_pair = watchers.level_id.pair.unwrap_or_default();

    (player_state_pair.current == PlayerState::Goal
        && player_state_pair.current != player_state_pair.old)
        || ((stage_pair.current == GameStage::StageSelect
            || stage_pair.current == GameStage::StageSelectPast)
            && stage_pair.old == GameStage::PacVillage)
}

fn split_checkpoints(checkpoints_pair: &Pair<i32>, settings: &Settings) -> bool {
    if !settings.split_checkpoint || !checkpoints_pair.changed() || checkpoints_pair.decreased() {
        return false;
    }

    let start_skip = match checkpoints_pair.old {
        -1 => 0,
        i => i,
    };
    let split_goal = checkpoints_pair.current;

    // skip how many checkpoints were skipped
    for _ in start_skip..(split_goal - 1) {
        timer::skip_split();
    }
    true
}

fn split_boss_phase(
    boss_phase_pair: &Pair<u32>,
    settings: &Settings,
    highest_phase: &mut u32,
) -> bool {
    if !settings.split_boss_phase || !boss_phase_pair.changed() || boss_phase_pair.decreased() {
        return false;
    }

    if boss_phase_pair.current > 1
        && boss_phase_pair.current == boss_phase_pair.old + 1
        && *highest_phase < boss_phase_pair.current
    {
        *highest_phase = boss_phase_pair.current;
        return true;
    }
    false
}

fn level_is_stage_select(stage: GameStage) -> bool {
    stage == GameStage::StageSelect
        || stage == GameStage::StageSelectPast
        || stage == GameStage::StageSelectSonic
}

fn level_is_boss_stage(stage: GameStage) -> bool {
    stage == GameStage::Stage1_4
        || stage == GameStage::Stage2_4
        || stage == GameStage::Stage3_4
        || stage == GameStage::Stage4_4
        || stage == GameStage::Stage5_4
        || stage == GameStage::Stage6_4
        || stage == GameStage::Stage6_5
        || stage == GameStage::Stage1_4Past
        || stage == GameStage::Stage2_4Past
        || stage == GameStage::Stage3_4Past
        || stage == GameStage::Stage4_4Past
        || stage == GameStage::Stage5_4Past
        || stage == GameStage::Stage6_4Past
        || stage == GameStage::StageSonic3
}
