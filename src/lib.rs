#![no_std]

use asr::{
    future::{next_tick, retry},
    game_engine::unity::il2cpp::{Image, Module, UnityPointer, Version},
    itoa, ryu,
    settings::{gui::Title, Gui},
    time::Duration,
    timer::{self},
    watcher::{Pair, Watcher},
    Process,
};
asr::async_main!(stable);
asr::panic_handler!();

async fn main() {
    let mut settings = Settings::register();

    asr::print_message("PACMAN REPAC TWOOOOOOO loaded");

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
                    update_loop(&process, &memory, &mut watchers);

                    // get memory values
                    let level = watchers.level_id.pair.unwrap_or(Pair::default()).current;
                    let time_trial_igt_pair =
                        watchers.time_trial_igt.pair.unwrap_or(Pair::default());
                    let time_trial_state_pair =
                        watchers.time_trial_state.pair.unwrap_or(Pair::default());
                    let mut buffer_int = itoa::Buffer::new();
                    let mut buffer_float = ryu::Buffer::new();

                    // vars display
                    asr::timer::set_variable("LevelEnum", buffer_int.format(level));
                    asr::timer::set_variable(
                        "Time Trial Timer",
                        buffer_float.format(time_trial_igt_pair.current),
                    );
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
                    if let Some(is_loading) = is_loading(&watchers, &settings) {
                        if is_loading {
                            timer::pause_game_time();
                            asr::timer::set_variable("Loading", "True");
                        } else {
                            timer::resume_game_time();
                            asr::timer::set_variable("Loading", "False");
                        }
                    }

                    match settings.timer_mode.current {
                        TimerMode::FullGame => {
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
                            timer::set_game_time(Duration::seconds_f64(
                                time_trial_igt_pair.current,
                            ));

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

                            if time_trial_state_pair.current == TimeTrialState::None {
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
    level_id: Watcher<u32>,
    time_trial_igt: Watcher<f64>,
    time_trial_state: Watcher<TimeTrialState>,
}

struct Memory {
    il2cpp_module: Module,
    game_assembly: Image,
    is_loading: UnityPointer<2>,
    level_id: UnityPointer<2>,
    time_trial_igt: UnityPointer<2>,
    time_trial_state: UnityPointer<2>,
}

impl Memory {
    fn init(game: &Process) -> Option<Self> {
        let il2cpp_module = Module::attach(game, Version::V2020)?;
        let game_assembly = il2cpp_module.get_default_image(game)?;

        let is_loading = UnityPointer::new("SceneManager", 1, &["s_sInstance", "m_bProcessing"]);
        let level_id = UnityPointer::new("SceneManager", 1, &["s_sInstance", "m_eCurrentScene"]);
        let time_trial_igt = UnityPointer::new("TimeAttackManager", 1, &["s_sInstance", "m_time"]);
        let time_trial_state =
            UnityPointer::new("TimeAttackManager", 1, &["s_sInstance", "m_step"]);

        // TODO investigate if it's possible to get the IGT from public class PlayTimeManager : SingletonBase<PlayTimeManager>
        // private List<PlayTimeManager.Timer> m_timerList;
        // this has all the timers... in a list... pointing to a dynamic class... and one of the fields it "time" which is what i need.......

        Some(Self {
            il2cpp_module,
            game_assembly,
            is_loading,
            level_id,
            time_trial_igt,
            time_trial_state,
        })
    }
}

fn update_loop(game: &Process, addresses: &Memory, watchers: &mut Watchers) {
    watchers.is_loading.update_infallible(
        addresses
            .is_loading
            .deref::<bool>(game, &addresses.il2cpp_module, &addresses.game_assembly)
            .unwrap_or_default(),
    );

    watchers.level_id.update_infallible(
        addresses
            .level_id
            .deref::<u32>(game, &addresses.il2cpp_module, &addresses.game_assembly)
            .unwrap_or_default(),
    );

    watchers.time_trial_igt.update_infallible(
        addresses
            .time_trial_igt
            .deref::<f64>(game, &addresses.il2cpp_module, &addresses.game_assembly)
            .unwrap_or_default(),
    );

    let time_trial_state_raw = addresses
        .time_trial_state
        .deref::<u32>(game, &addresses.il2cpp_module, &addresses.game_assembly)
        .unwrap_or_default();
    watchers
        .time_trial_state
        .update_infallible(match time_trial_state_raw {
            0 => TimeTrialState::None,
            1 => TimeTrialState::ReadyInit,
            2 => TimeTrialState::ReadyWait,
            3 => TimeTrialState::TA,
            4 => TimeTrialState::Pause,
            5 => TimeTrialState::End,
            _ => TimeTrialState::Unknown,
        });
}

fn is_loading(watchers: &Watchers, _settings: &Settings) -> Option<bool> {
    Some(watchers.is_loading.pair?.current)
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

    level_pair.current == 6 && level_pair.old == 4 && settings.start_new_game
}

fn split(watchers: &Watchers, settings: &Settings) -> bool {
    let level_pair = if let Some(pair) = &watchers.level_id.pair {
        pair
    } else {
        return false;
    };

    if !level_pair.changed() {
        return false;
    }

    match level_pair.current {
        9 => true && settings.split_on_level_complete,
        _ => false,
    }
}
