use crate::{Settings, TimeTrialState, TimerMode, Watchers};
use asr::{
    game_engine::unity::il2cpp::{Image, Module, UnityPointer, Version},
    Process,
};

pub struct Memory {
    il2cpp_module: Module,
    game_assembly: Image,
    is_loading: UnityPointer<2>,
    // is_loading is not fully accurate, there is an animation at the loading screen that depends on the frame rate and is_loading is set to false during that
    // the solution found was using the an UI param to complement is_loading
    loadscreen_ui_pointer: UnityPointer<2>,
    level_id: UnityPointer<2>,
    checkpoint: UnityPointer<2>,
    time_trial_igt: UnityPointer<2>,
    time_trial_state: UnityPointer<2>,
    time_trial_bonus_list_pointer: UnityPointer<2>,
    spooky_qte_success: UnityPointer<3>,
    tocman_hp: UnityPointer<3>,
    tocman_state: UnityPointer<3>,
}

impl Memory {
    pub fn init(game: &Process) -> Option<Self> {
        let il2cpp_module = Module::attach(game, Version::V2020)?;
        let game_assembly = il2cpp_module.get_default_image(game)?;

        let is_loading = UnityPointer::new("SceneManager", 1, &["s_sInstance", "m_bProcessing"]);
        let level_id = UnityPointer::new("SceneManager", 1, &["s_sInstance", "m_eCurrentScene"]);
        let checkpoint = UnityPointer::new(
            "StageStateManager",
            1,
            &["s_sInstance", "m_checkPointPriority"],
        );
        let time_trial_igt = UnityPointer::new("TimeAttackManager", 1, &["s_sInstance", "m_time"]);
        let time_trial_state =
            UnityPointer::new("TimeAttackManager", 1, &["s_sInstance", "m_step"]);
        let time_trial_bonus_list_pointer =
            UnityPointer::new("TimeAttackManager", 1, &["s_sInstance", "m_bonusTimeList"]);
        let loadscreen_ui_pointer =
            UnityPointer::new("SystemUIRoot", 1, &["s_sInstance", "m_sLoadingUI"]);
        let spooky_qte_success =
            UnityPointer::new("BossSpooky", 3, &["s_sInstance", "m_qteSuccess"]);
        let tocman_hp = UnityPointer::new("BossTocman", 2, &["s_sInstance", "m_life"]);
        let tocman_state = UnityPointer::new("BossTocman", 2, &["s_sInstance", "m_state"]);

        Some(Self {
            il2cpp_module,
            game_assembly,
            is_loading,
            loadscreen_ui_pointer,
            checkpoint,
            level_id,
            time_trial_igt,
            time_trial_state,
            time_trial_bonus_list_pointer,
            spooky_qte_success,
            tocman_hp,
            tocman_state,
        })
    }
}

pub fn update_watchers(
    game: &Process,
    addresses: &Memory,
    watchers: &mut Watchers,
    settings: &Settings,
) {
    let level_id = addresses
        .level_id
        .deref::<u32>(game, &addresses.il2cpp_module, &addresses.game_assembly)
        .unwrap_or_default()
        .into();
    watchers.level_id.update_infallible(level_id);

    let checkpoint = addresses
        .checkpoint
        .deref::<i32>(game, &addresses.il2cpp_module, &addresses.game_assembly)
        .unwrap_or_default();
    watchers.checkpoint.update_infallible(checkpoint);

    match settings.timer_mode.current {
        TimerMode::TimeTrial => {
            let bonus_list_address_res = addresses.time_trial_bonus_list_pointer.deref::<u64>(
                game,
                &addresses.il2cpp_module,
                &addresses.game_assembly,
            );

            let time_trial_igt = addresses
                .time_trial_igt
                .deref::<f64>(game, &addresses.il2cpp_module, &addresses.game_assembly)
                .unwrap_or_default();
            watchers.time_trial_igt.update_infallible(time_trial_igt);

            let time_trial_state_raw = addresses
                .time_trial_state
                .deref::<u32>(game, &addresses.il2cpp_module, &addresses.game_assembly)
                .unwrap_or_default();
            let time_trial_state = match time_trial_state_raw {
                0 => TimeTrialState::None,
                1 => TimeTrialState::ReadyInit,
                2 => TimeTrialState::ReadyWait,
                3 => TimeTrialState::TA,
                4 => TimeTrialState::Pause,
                5 => TimeTrialState::End,
                _ => TimeTrialState::Unknown,
            };
            watchers
                .time_trial_state
                .update_infallible(time_trial_state);

            if let Ok(list_pointer) = bonus_list_address_res {
                let time_trial_bonus = calculate_time_bonus(game, list_pointer);
                watchers
                    .time_trial_bonus_time
                    .update_infallible(time_trial_bonus);
                asr::timer::set_variable_int("Time Trial Total Bonus", time_trial_bonus);
            }

            asr::timer::set_variable_float("Time Trial Timer", time_trial_igt);
            asr::timer::set_variable(
                "Time Trial State",
                match time_trial_state {
                    TimeTrialState::None => "None",
                    TimeTrialState::ReadyInit => "Ready_Init",
                    TimeTrialState::ReadyWait => "Ready_Wait",
                    TimeTrialState::TA => "TA",
                    TimeTrialState::Pause => "Pause",
                    TimeTrialState::End => "End",
                    TimeTrialState::Unknown => "Unknown",
                },
            );
        }
        TimerMode::FullGame => {
            watchers.is_loading.update_infallible(
                addresses
                    .is_loading
                    .deref::<bool>(game, &addresses.il2cpp_module, &addresses.game_assembly)
                    .unwrap_or_default(),
            );

            // get the loading animation progress from the UI for a more accurate (normal) level start time
            let loading_ui_add_res = addresses.loadscreen_ui_pointer.deref::<u64>(
                game,
                &addresses.il2cpp_module,
                &addresses.game_assembly,
            );
            if let Ok(ui_add) = loading_ui_add_res {
                //asr::timer::set_variable_int("m_sLoadingUI", ui_add);

                // 0x4C: m_fProgPrev
                let load_progress_pc = game.read::<f32>(ui_add + 0x4C).unwrap_or_default();
                watchers
                    .load_ui_progress
                    .update_infallible(load_progress_pc);
                asr::timer::set_variable_float("UI Load Anim Progress", load_progress_pc);
            }

            let spooky_qte_success = addresses
                .spooky_qte_success
                .deref::<bool>(game, &addresses.il2cpp_module, &addresses.game_assembly)
                .unwrap_or_default();
            watchers
                .spooky_qte_success
                .update_infallible(spooky_qte_success);

            let tocman_hp = addresses
                .tocman_hp
                .deref::<i32>(game, &addresses.il2cpp_module, &addresses.game_assembly)
                .unwrap_or_default();
            watchers.tocman_hp.update_infallible(tocman_hp);

            let tocman_state = addresses
                .tocman_state
                .deref::<u32>(game, &addresses.il2cpp_module, &addresses.game_assembly)
                .unwrap_or_default();
            watchers.tocman_state.update_infallible(tocman_state);

            asr::timer::set_variable("LevelEnum", level_id.to_string());
            asr::timer::set_variable_int("Checkpoint", checkpoint);
            asr::timer::set_variable_int("Toc-Man HP", tocman_hp);
            asr::timer::set_variable_int("Toc-Man State", tocman_state);
        }
    }
}

fn calculate_time_bonus(game: &Process, bonus_list_pointer: u64) -> u32 {
    // this is a list pointer, it is an object with the data but not 100% straightforward

    // relevant data in this list object:
    // 0x10: pointer to the actual array, which is also an object, not just raw data
    // 0x18: actual length of array (not what's allocated)

    let items_pointer_res = game.read::<u64>(bonus_list_pointer + 0x10);

    let items_pointer = match items_pointer_res {
        Ok(pointer) => pointer,
        Err(_) => return 0,
    };

    let list_size = game
        .read::<u32>(bonus_list_pointer + 0x18)
        .unwrap_or_default();

    // now in the actual array
    // 0x20: all the data in order, thankfully it's just u32 ints in this case
    let mut total_bonus = 0;
    for i in 0..list_size {
        total_bonus += game
            .read::<u32>(items_pointer + 0x20 + (0x4 * i as u64))
            .unwrap_or_default();
    }

    total_bonus
}
