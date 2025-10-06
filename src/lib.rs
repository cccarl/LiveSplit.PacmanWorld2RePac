#![no_std]

mod memory;
mod stages;

use asr::{
    future::{next_tick, retry},
    settings::{gui::Title, Gui},
    time::Duration,
    timer::{self},
    watcher::{Pair, Watcher},
    Process,
};
use memory::{update_watchers, Memory};
use stages::Stages;

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

                // Perform memory scanning to look for the addresses we need
                let memory = retry(|| Memory::init(&process)).await;
                loop {
                    // MAIN LOOP
                    settings.update();
                    update_watchers(&process, &memory, &mut watchers, &settings);

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

                    match settings.timer_mode.current {
                        TimerMode::FullGame => {
                            if is_loading_pair.current
                                || (load_ui_progress_pair.current > 0.0
                                    && load_ui_progress_pair.current < 1.0)
                            {
                                timer::pause_game_time();
                                asr::timer::set_variable("Loading", "True");
                            } else {
                                timer::resume_game_time();
                                asr::timer::set_variable("Loading", "False");
                            }

                            if start(&watchers, &settings) {
                                if settings.reset_on_file_creation {
                                    timer::reset();
                                }
                                timer::start();
                                timer::set_game_time(Duration::new(3, 350_000_000));
                            }

                            if split(&watchers, &settings) {
                                timer::split();
                            }
                        }
                        TimerMode::TimeTrial => {
                            timer::pause_game_time();

                            if settings.time_trial_discount_bonus {
                                timer::set_game_time(Duration::seconds_f64(
                                    time_trial_igt_pair.current
                                        - (time_trial_bonus_pair.current as f64),
                                ));
                            } else {
                                timer::set_game_time(Duration::seconds_f64(
                                    time_trial_igt_pair.current,
                                ));
                            }

                            if time_trial_state_pair.old == TimeTrialState::None
                                && time_trial_state_pair.current == TimeTrialState::TA
                            {
                                timer::start();
                            }

                            // TODO add checkpoint splits options
                            if time_trial_state_pair.old == TimeTrialState::TA
                                && time_trial_state_pair.current == TimeTrialState::End
                            {
                                timer::split();
                            }

                            if time_trial_state_pair.current == TimeTrialState::None
                                && time_trial_igt_pair.current != time_trial_igt_pair.old
                            {
                                timer::reset();
                            }
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
    /// Time Trial
    TimeTrial,
}

#[derive(Gui)]
struct Settings {
    /// LiveSplit Timer Mode
    _timer_mode: Title,

    /// Pick a Mode
    timer_mode: Pair<TimerMode>,

    /// Start Options
    _title_start: Title,

    /// New File
    #[default = true]
    start_new_game: bool,

    /// Split Options
    _title_split: Title,

    /// Level Exit
    #[default = true]
    split_on_level_complete: bool,

    /// Spooky Defeat
    #[default = true]
    split_spooky_qte: bool,

    /// Toc-Man Defeat
    #[default = true]
    split_tocman: bool,

    /// Reset Options
    _title_reset: Title,

    /// New File
    #[default = true]
    reset_on_file_creation: bool,

    /// Misc
    _misc_title: Title,

    /// Discount Bonus Time on Time Trials
    #[default = true]
    time_trial_discount_bonus: bool,
}

#[derive(Clone, Copy, PartialEq, Default)]
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

#[derive(Default)]
struct Watchers {
    is_loading: Watcher<bool>,
    load_ui_progress: Watcher<f32>,
    level_id: Watcher<Stages>,
    checkpoint: Watcher<i32>,
    time_trial_igt: Watcher<f64>,
    time_trial_state: Watcher<TimeTrialState>,
    time_trial_bonus_time: Watcher<u32>,
    spooky_qte_success: Watcher<bool>,
    tocman_hp: Watcher<i32>,
    tocman_state: Watcher<u32>,
}

fn start(watchers: &Watchers, settings: &Settings) -> bool {
    let level_pair = if let Some(pair) = &watchers.level_id.pair {
        pair
    } else {
        return false;
    };

    if level_pair.current == level_pair.old {
        return false;
    }

    level_pair.current == Stages::Movie
        && level_pair.old == Stages::Title
        && settings.start_new_game
}

fn split(watchers: &Watchers, settings: &Settings) -> bool {
    // level exit split
    let level_pair = watchers.level_id.pair.unwrap_or_default();

    if level_pair.current != level_pair.old {
        match level_pair.current {
            Stages::StageSelect => return true && settings.split_on_level_complete,
            _ => return false,
        }
    };

    // spooky qte final split
    let spooky_pair = watchers.spooky_qte_success.pair.unwrap_or_default();
    if spooky_pair.changed() && spooky_pair.current && settings.split_spooky_qte {
        return true;
    }

    // tocman final hit split
    let tocman_hp_pair = watchers.tocman_hp.pair.unwrap_or_default();
    let tocman_state_pair = watchers.tocman_state.pair.unwrap_or_default();
    return tocman_hp_pair.changed()
        && tocman_hp_pair.current == 0
        && tocman_state_pair.current == 3
        && settings.split_tocman;
}
