use crate::{Settings, TimeTrialState, TimerMode, Watchers};
use asr::{
    game_engine::unity::il2cpp::{Image, Module, UnityPointer, Version},
    Process,
};

pub struct Memory {
    il2cpp_module: Module,
    game_assembly: Image,
    is_loading: UnityPointer<2>,
    level_id: UnityPointer<2>,
    time_trial_igt: UnityPointer<2>,
    time_trial_state: UnityPointer<2>,
    time_trial_bonus_list_pointer: UnityPointer<2>,
    save_data_manager_pointer: UnityPointer<2>,
    save_slot: UnityPointer<2>,
    timer_list_pointer: UnityPointer<2>,
}

impl Memory {
    pub fn init(game: &Process) -> Option<Self> {
        let il2cpp_module = Module::attach(game, Version::V2020)?;
        let game_assembly = il2cpp_module.get_default_image(game)?;

        let is_loading = UnityPointer::new("SceneManager", 1, &["s_sInstance", "m_bProcessing"]);
        let level_id = UnityPointer::new("SceneManager", 1, &["s_sInstance", "m_eCurrentScene"]);
        let time_trial_igt = UnityPointer::new("TimeAttackManager", 1, &["s_sInstance", "m_time"]);
        let time_trial_state =
            UnityPointer::new("TimeAttackManager", 1, &["s_sInstance", "m_step"]);
        let time_trial_bonus_list_pointer =
            UnityPointer::new("TimeAttackManager", 1, &["s_sInstance", "m_bonusTimeList"]);

        let save_data_manager_pointer =
            UnityPointer::new("SaveDataManager", 1, &["s_sInstance", "m_implement"]);

        let save_slot =
            UnityPointer::new("GameStateManager", 1, &["s_sInstance", "m_currentSaveSlot"]);

        // timer thats running for the igt
        let timer_list_pointer =
            UnityPointer::new("PlayTimeManager", 1, &["s_sInstance", "m_timerList"]);

        Some(Self {
            il2cpp_module,
            game_assembly,
            is_loading,
            level_id,
            time_trial_igt,
            time_trial_state,
            time_trial_bonus_list_pointer,
            save_data_manager_pointer,
            save_slot,
            timer_list_pointer,
        })
    }
}

struct IgtCalculated {
    hour: i32,
    minute: i32,
    second: i32,
}

pub fn update_watchers(
    game: &Process,
    addresses: &Memory,
    watchers: &mut Watchers,
    settings: &Settings,
) {
    watchers.is_loading.update_infallible(
        addresses
            .is_loading
            .deref::<bool>(game, &addresses.il2cpp_module, &addresses.game_assembly)
            .unwrap_or_default(),
    );

    let level_id = addresses
        .level_id
        .deref::<u32>(game, &addresses.il2cpp_module, &addresses.game_assembly)
        .unwrap_or_default();
    watchers.level_id.update_infallible(level_id.into());

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

    match settings.timer_mode.current {
        TimerMode::TimeTrial => {
            let bonus_list_address_res = addresses.time_trial_bonus_list_pointer.deref::<u64>(
                game,
                &addresses.il2cpp_module,
                &addresses.game_assembly,
            );

            if let Ok(list_pointer) = bonus_list_address_res {
                watchers
                    .time_trial_bonus_list_pointer
                    .update_infallible(list_pointer);

                watchers
                    .time_trial_bonus_time
                    .update_infallible(calculate_time_bonus(game, list_pointer));
            } else {
            }
        }
        TimerMode::FullGame => {}
    }

    // TODO move to full game runs only
    let save_slot = addresses
        .save_slot
        .deref::<i32>(game, &addresses.il2cpp_module, &addresses.game_assembly)
        .unwrap_or_default();
    watchers.save_slot.update_infallible(save_slot);
    let save_data_ptr_res = addresses.save_data_manager_pointer.deref::<u64>(
        game,
        &addresses.il2cpp_module,
        &addresses.game_assembly,
    );

    if let Ok(list_save_data_pointer) = save_data_ptr_res {
        watchers
            .save_data_pointer
            .update_infallible(list_save_data_pointer);
        let igt_update = calculate_main_igt(game, list_save_data_pointer, save_slot);
        watchers.save_data_hour.update_infallible(igt_update.hour);
        watchers
            .save_data_minute
            .update_infallible(igt_update.minute);
        watchers
            .save_data_second
            .update_infallible(igt_update.second);
    }

    let timers_list_pointer = addresses.timer_list_pointer
        .deref::<u64>(game, &addresses.il2cpp_module, &addresses.game_assembly)
        .unwrap_or_default();
    let curr_timer = calculate_running_timer(game, timers_list_pointer);
    watchers.current_timer.update_infallible(curr_timer);

}

fn calculate_time_bonus(game: &Process, bonus_list_pointer: u64) -> u32 {
    // this is a unity list pointer, it is an object with the data but not 100% straightforward

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

fn calculate_main_igt(
    game: &Process,
    save_manager_pointer: u64,
    current_slot: i32,
) -> IgtCalculated {
    let mut igt = IgtCalculated {
        hour: 0,
        minute: 0,
        second: 0,
    };

    // pointer in param: SaveDataManager.m_implement
    // 0x10: ISaveDataManagerImplement.m_saveData
    // 0x38: SaveData.m_subDataList (array)
    // 0x20 + saveslot * 8: SaveDataProgData.m_base (struct)
    // ----> 0x28: SDataBase.m_iPlayHours
    //       0x2C: SDataBase.m_iPlayMinutes
    //       0x30: SDataBase.m_iPlaySeconds
    let s_data_base_obj_ptr = game
        .read_pointer_path::<u64>(
            save_manager_pointer,
            asr::PointerSize::Bit64,
            &[0x10, 0x38, 0x20 + (current_slot as u64 * 0x8)],
        )
        .unwrap_or_default();

    igt.hour = game
        .read::<i32>(s_data_base_obj_ptr + 0x28)
        .unwrap_or_default();
    igt.minute = game
        .read::<i32>(s_data_base_obj_ptr + 0x2C)
        .unwrap_or_default();
    igt.second = game
        .read::<i32>(s_data_base_obj_ptr + 0x30)
        .unwrap_or_default();

    igt
}

// old attempt at getting the igt, but MAYBE this is the timer thats used for incrementing the total slowly
fn calculate_running_timer(game: &Process, timers_list_pointer: u64) -> f64 {
    // same as the time bonus as a base, but it's a list of objects instead (pointers instead of numbers)
    let items_pointer_res = game.read::<u64>(timers_list_pointer + 0x10);
    let items_pointer = match items_pointer_res {
        Ok(pointer) => pointer,
        Err(_) => return 0.0,
    };

    let list_size = game
        .read::<u32>(timers_list_pointer + 0x18)
        .unwrap_or_default();

    asr::timer::set_variable_int("Timers active", list_size);

    // each object is a "Timer" class, relevant fields:
    // 0x10 timer kind
    // 0x18 time
    let mut total_igt: f64 = 0.0;
    for i in 0..list_size {
        let timer_obj_address = game
            .read::<u64>(items_pointer + 0x20 + (0x8 * i as u64))
            .unwrap_or_default();

        let timer_kind = game
            .read::<u64>(timer_obj_address + 0x10)
            .unwrap_or_default();

        // we only care about type 1, "All"
        if timer_kind != 1 {
            continue;
        }

        let time_in_obj = game
            .read::<f64>(timer_obj_address + 0x18)
            .unwrap_or_default();
        total_igt += time_in_obj;
    }

    total_igt
}
