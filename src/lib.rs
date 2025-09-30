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
                    let level = watchers.level_id.pair.unwrap_or(Pair::default()).current;
                    let time_trial_igt_pair =
                        watchers.time_trial_igt.pair.unwrap_or(Pair::default());
                    let time_trial_state_pair =
                        watchers.time_trial_state.pair.unwrap_or(Pair::default());
                    let time_trial_bonus_list_pair = watchers
                        .time_trial_bonus_list_pointer
                        .pair
                        .unwrap_or(Pair::default());
                    let time_trial_bonus_pair = watchers
                        .time_trial_bonus_time
                        .pair
                        .unwrap_or(Pair::default());
                    let igt_address = watchers.timers_list_pointer.pair.unwrap_or(Pair::default());
                    let save_slot_pair = watchers.save_slot.pair.unwrap_or(Pair::default());
                    let save_data_address =
                        watchers.save_data_pointer.pair.unwrap_or(Pair::default());
                    let igt_hours = watchers.save_data_hour.pair.unwrap_or(Pair::default());
                    let igt_minutes = watchers.save_data_minute.pair.unwrap_or(Pair::default());
                    let igt_seconds = watchers.save_data_second.pair.unwrap_or(Pair::default());
                    let curr_timer_pair = watchers.current_timer.pair.unwrap_or(Pair::default());

                    // vars display
                    asr::timer::set_variable("LevelEnum", level.to_string());
                    asr::timer::set_variable_float("Time Trial Timer", time_trial_igt_pair.current);
                    asr::timer::set_variable(
                        "Time Trial State",
                        match time_trial_state_pair.current {
                            TimeTrialState::None => "None",
                            TimeTrialState::ReadyInit => "Ready_Init",
                            TimeTrialState::ReadyWait => "Ready_Wait",
                            TimeTrialState::TA => "TA",
                            TimeTrialState::Pause => "Pause",
                            TimeTrialState::End => "End",
                            TimeTrialState::Unknown => "Unknown",
                        },
                    );
                    asr::timer::set_variable_int(
                        "Time Trial Bonus List Address",
                        time_trial_bonus_list_pair.current,
                    );
                    asr::timer::set_variable_int(
                        "Time Trial Total Bonus",
                        time_trial_bonus_pair.current,
                    );
                    asr::timer::set_variable_int("IGT List Address", igt_address.current);
                    asr::timer::set_variable_int("Save Data Address", save_data_address.current);
                    asr::timer::set_variable_int("Save Slot", save_slot_pair.current);

                    match settings.timer_mode.current {
                        TimerMode::FullGame => {

                            asr::timer::pause_game_time();
                            asr::timer::set_game_time(Duration::seconds_f64(
                                curr_timer_pair.current
                                    + (igt_hours.current * 3600
                                        + igt_minutes.current * 60
                                        + igt_seconds.current)
                                        as f64,
                            ));

                            if start(&watchers, &settings) {
                                if settings.reset_on_file_creation {
                                    timer::reset();
                                }
                                timer::start();
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

    /// Start On New File
    #[default = true]
    start_new_game: bool,

    /// Split Options
    _title_split: Title,

    /// Split On Level Complete
    #[default = true]
    split_on_level_complete: bool,

    /// Reset Options
    _title_reset: Title,

    /// Reset On New File
    #[default = true]
    reset_on_file_creation: bool,

    /// Misc
    _misc_title: Title,

    /// Discount Bonus Time
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
    level_id: Watcher<Stages>,
    time_trial_igt: Watcher<f64>,
    time_trial_state: Watcher<TimeTrialState>,
    time_trial_bonus_list_pointer: Watcher<u64>,
    time_trial_bonus_time: Watcher<u32>, // calculated bonus form the unity list with each timer
    timers_list_pointer: Watcher<u64>,
    save_data_pointer: Watcher<u64>,
    save_slot: Watcher<i32>,
    save_data_hour: Watcher<i32>,
    save_data_minute: Watcher<i32>,
    save_data_second: Watcher<i32>,
    current_timer: Watcher<f64>,
}


// TODO use for load remover if we decide to time the game with it
fn is_loading(watchers: &Watchers, _settings: &Settings) -> Option<bool> {
    Some(watchers.is_loading.pair?.current)
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
    let level_pair = if let Some(pair) = &watchers.level_id.pair {
        pair
    } else {
        return false;
    };

    if level_pair.current == level_pair.old {
        return false;
    }

    match level_pair.current {
        Stages::StageSelect => true && settings.split_on_level_complete,
        _ => false,
    }
}
