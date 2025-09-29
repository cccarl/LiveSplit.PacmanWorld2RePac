#![no_std]

use asr::{
    future::{next_tick, retry},
    game_engine::unity::il2cpp::{Image, Module, UnityPointer, Version},
    itoa,
    settings::{gui::Title, Gui},
    time::Duration,
    timer::{self, TimerState},
    watcher::{Pair, Watcher},
    Process,
};
asr::async_main!(stable);
asr::panic_handler!();

async fn main() {
    // TODO: Set up some general state and settings.
    let mut settings = Settings::register();

    asr::print_message("Hello, World!");

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

                    if start(&watchers, &settings) {
                        if settings.reset_on_file_creation {
                            timer::reset();
                        }
                        timer::start();
                    }

                    if split(&watchers, &settings) {
                        timer::split();
                    }

                    if let Some(is_loading) = is_loading(&watchers, &settings) {
                        if is_loading {
                            timer::pause_game_time();
                            asr::timer::set_variable("Loading", "True");
                        } else {
                            timer::resume_game_time();
                            asr::timer::set_variable("Loading", "False");
                        }
                    }

                    let level = watchers.level_id.pair.unwrap_or(Pair::default()).current;
                    let mut buffer = itoa::Buffer::new();
                    asr::timer::set_variable("LevelEnum", buffer.format(level));

                    next_tick().await;
                }
            })
            .await;
    }
}

#[derive(Gui)]
struct Settings {

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

#[derive(Default)]
struct Watchers {
    is_loading: Watcher<bool>,
    level_id: Watcher<u32>,
    /* level_id_unfiltered: Watcher<u32>,
    tocman_qte: Watcher<bool>, */
}

struct Memory {
    il2cpp_module: Module,
    game_assembly: Image,
    is_loading: UnityPointer<2>,
    level_id: UnityPointer<2>,
    /* is_loading_2: UnityPointer<2>,
    tocman_qte: UnityPointer<2>, */
}

impl Memory {
    fn init(game: &Process) -> Option<Self> {
        let il2cpp_module = Module::attach(game, Version::V2020)?;
        let game_assembly = il2cpp_module.get_default_image(game)?;

        let is_loading = UnityPointer::new("SceneManager", 1, &["s_sInstance", "m_bProcessing"]);
        let level_id = UnityPointer::new("SceneManager", 1, &["s_sInstance", "m_eCurrentScene"]);

        /* let is_loading_2 = UnityPointer::new("GameStateManager", 1, &["s_sInstance", "loadScr"]);
        let tocman_qte = UnityPointer::new("BossTocman", 1, &["s_sInstance", "m_qteSuccess"]); */

        Some(Self {
            il2cpp_module,
            game_assembly,
            is_loading,
            level_id,
            /* is_loading_2,
            tocman_qte, */
        })
    }
}

fn update_loop(game: &Process, addresses: &Memory, watchers: &mut Watchers) {
    watchers.is_loading.update_infallible(
        addresses
            .is_loading
            .deref::<bool>(game, &addresses.il2cpp_module, &addresses.game_assembly)
            .unwrap_or_default(), /* || addresses
                                  .is_loading_2
                                  .deref::<u64>(game, &addresses.il2cpp_module, &addresses.game_assembly)
                                  .unwrap_or_default()
                                  != 0, */
    );

    let cur_level = addresses
        .level_id
        .deref::<u32>(game, &addresses.il2cpp_module, &addresses.game_assembly)
        .unwrap_or_default();

    watchers.level_id.update_infallible({ cur_level });

    /* watchers
    .tocman_qte
    .update_infallible(addresses.tocman_qte.deref(game, &addresses.il2cpp_module, &addresses.game_assembly).unwrap_or_default()); */
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
        9 => {
            true && settings.split_on_level_complete
        }
        _ => false
    }
}

