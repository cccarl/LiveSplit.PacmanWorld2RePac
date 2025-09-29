use asr::{
    game_engine::unity::il2cpp::{Image, Module, UnityPointer, Version},
    Process,
};
use crate::{Settings, TimeTrialState, TimerMode, Watchers};

pub struct Memory {
    il2cpp_module: Module,
    game_assembly: Image,
    is_loading: UnityPointer<2>,
    level_id: UnityPointer<2>,
    time_trial_igt: UnityPointer<2>,
    time_trial_state: UnityPointer<2>,
    time_trial_bonus_list_pointer: UnityPointer<2>,
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
        let time_trial_bonus_list =
            UnityPointer::new("TimeAttackManager", 1, &["s_sInstance", "m_bonusTimeList"]);

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
            time_trial_bonus_list_pointer: time_trial_bonus_list,
        })
    }
}

pub fn update_watchers(game: &Process, addresses: &Memory, watchers: &mut Watchers, settings: &Settings) {
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

    if settings.timer_mode.current == TimerMode::TimeTrial {
        let bonus_list_address_res = addresses.time_trial_bonus_list_pointer.deref::<u64>(
            game,
            &addresses.il2cpp_module,
            &addresses.game_assembly,
        );

        if let Ok(list_ponter) = bonus_list_address_res {
            watchers
                .time_trial_bonus_list_pointer
                .update_infallible(list_ponter);

            watchers
                .time_trial_bonus_time
                .update_infallible(calculate_time_bonus(game, list_ponter));
        }
    }
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
