use crate::{
    stages::Stages, PlayerState, Settings, StageState, TimeTrialState, TimerMode, Watchers,
};
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
    // conviniently all bosses are inherited from "BossBase" and use "m_state" as their current phase
    // only detail is that their offsets in the pointer path are different so they are refreshed on a level change
    boss_state: UnityPointer<3>,
    players_array: UnityPointer<3>,
    player_state_offset: Option<u32>,
    stage_manager_state: UnityPointer<3>,
    // WIP
    /* title_scene_step: UnityPointer<3>, */
}

impl Memory {
    pub fn init(game: &Process) -> Option<Self> {
        let il2cpp_module = Module::attach(game, Version::V2020)?;
        let game_assembly = il2cpp_module.get_default_image(game)?;

        let is_loading = UnityPointer::new("SceneManager", 1, &["s_sInstance", "m_bProcessing"]);
        let loadscreen_ui_pointer =
            UnityPointer::new("SystemUIRoot", 1, &["s_sInstance", "m_sLoadingUI"]);
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
        let spooky_qte_success =
            UnityPointer::new("BossSpooky", 3, &["s_sInstance", "m_qteSuccess"]);
        let boss_state = UnityPointer::new("BossBase", 1, &["s_sInstance", "m_state"]);
        let players_array = UnityPointer::new("PlayerManager", 2, &["s_sInstance", "m_players"]);
        let stage_manager_state = UnityPointer::new("StageManager", 2, &["s_sInstance", "m_step"]);

        // init the player state offset in the PlayerPacman class
        let pacman_class_opt = game_assembly.get_class(&game, &il2cpp_module, "PlayerPacman");
        let player_state_offset = if let Some(player_class) = pacman_class_opt {
            let offset_opt = player_class.get_field_offset(&game, &il2cpp_module, "m_step");
            offset_opt
        } else {
            None
        };

        // TODO cope for a better autostart
        // GameLevelSelect seems to be the UI to pick difficulty but theres no reference to it on a field...
        // reding "private GameLevelSelect.EStep m_step;" would be as perfect of a start as it could be

        // GameUI (singleton) has "private GameObject m_goGameLevelPrefab;", does this lead to the GameLevelSelect somehow?

        // 2nd best thing thats easy to use but might as well keep using the intro level video if its not frame perfect, this is like half a second off still
        /* let title_scene_step = UnityPointer::new("TitleScene", 2, &["s_sInstance", "m_step"]); */

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
            boss_state,
            players_array,
            player_state_offset,
            stage_manager_state,
            /* title_scene_step, */
        })
    }

    pub fn refresh_pointers(&mut self) {
        self.boss_state = UnityPointer::new("BossBase", 1, &["s_sInstance", "m_state"]);
        self.stage_manager_state = UnityPointer::new("StageManager", 2, &["s_sInstance", "m_step"]);
    }

    pub fn refresh_player_state_offset(&mut self, game: &Process) {
        let pacman_class_opt =
            self.game_assembly
                .get_class(&game, &self.il2cpp_module, "PlayerPacman");
        if let Some(pac) = pacman_class_opt {
            let offset_opt = pac.get_field_offset(&game, &self.il2cpp_module, "m_step");
            self.player_state_offset = offset_opt;
        } else {
            self.player_state_offset = None;
        };
    }
}

pub fn update_watchers(
    game: &Process,
    addresses: &mut Memory,
    watchers: &mut Watchers,
    settings: &Settings,
) {
    /* let title_scene_step = addresses
        .title_scene_step
        .deref::<u64>(game, &addresses.il2cpp_module, &addresses.game_assembly)
        .unwrap_or_default();
    asr::timer::set_variable_int("TITLE STEP", title_scene_step); */

    let level_id = addresses
        .level_id
        .deref::<u32>(game, &addresses.il2cpp_module, &addresses.game_assembly)
        .unwrap_or(100_000)
        .into();
    watchers.level_id.update_infallible(level_id);

    // refresh pointer paths on level change
    let curr_level_pair = watchers.level_id.pair.unwrap_or_default();
    if curr_level_pair.current != curr_level_pair.old {
        addresses.refresh_pointers();
    }

    let is_loading = addresses
        .is_loading
        .deref::<bool>(game, &addresses.il2cpp_module, &addresses.game_assembly)
        .unwrap_or_default();
    watchers.is_loading.update_infallible(is_loading);

    let checkpoint = addresses
        .checkpoint
        .deref::<i32>(game, &addresses.il2cpp_module, &addresses.game_assembly)
        .unwrap_or_default();
    watchers.checkpoint.update_infallible(checkpoint);

    let players_array_pointer_res = addresses.players_array.deref::<u64>(
        game,
        &addresses.il2cpp_module,
        &addresses.game_assembly,
    );
    if let Ok(players_array_pointer) = players_array_pointer_res {
        let player_state = get_player1_state(game, players_array_pointer, addresses);
        watchers.player_state.update_infallible(player_state);
        asr::timer::set_variable("Player State", player_state_to_string(player_state));
    }

    if is_loading {
        asr::timer::set_variable("Loading", "True");
    } else {
        asr::timer::set_variable("Loading", "False");
    }
    asr::timer::set_variable("LevelEnum", level_id.to_string());
    asr::timer::set_variable_int("Checkpoint", checkpoint);

    match settings.timer_mode.current {
        TimerMode::IL => {
            if level_id != Stages::StageSelect && level_id != Stages::StageSelectPast {
                let stage_manager_state_int = addresses
                    .stage_manager_state
                    .deref::<u32>(game, &addresses.il2cpp_module, &addresses.game_assembly)
                    .unwrap_or_default();
                let stage_manager_state = get_stage_manager_state(stage_manager_state_int);
                watchers.stage_state.update_infallible(stage_manager_state);

                asr::timer::set_variable(
                    "Stage Manager State",
                    stage_state_to_string(stage_manager_state),
                );
            }

            if settings.split_boss_phase {
                let boss_state = get_boss_state(&game, &addresses, &level_id);
                watchers.boss_state.update_infallible(boss_state);
                asr::timer::set_variable_int("Boss State", boss_state);
            }
        }
        TimerMode::FullGame => {
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

            if level_id == Stages::Stage6_4 {
                let spooky_qte_success = addresses
                    .spooky_qte_success
                    .deref::<bool>(game, &addresses.il2cpp_module, &addresses.game_assembly)
                    .unwrap_or_default();
                watchers
                    .spooky_qte_success
                    .update_infallible(spooky_qte_success);
                asr::timer::set_variable(
                    "Spooky QTE Complete",
                    match spooky_qte_success {
                        true => "Yes",
                        false => "No",
                    },
                );
            }

            if level_id == Stages::Stage6_5 && settings.split_tocman {
                let boss_state = get_boss_state(&game, &addresses, &level_id);
                watchers.boss_state.update_infallible(boss_state);
                asr::timer::set_variable_int("Boss State", boss_state);
            }
        }
        TimerMode::TimeTrial => {
            let bonus_list_address_res = addresses.time_trial_bonus_list_pointer.deref::<u64>(
                game,
                &addresses.il2cpp_module,
                &addresses.game_assembly,
            );

            if let Ok(list_pointer) = bonus_list_address_res {
                let time_trial_bonus = calculate_time_bonus(game, list_pointer);
                watchers
                    .time_trial_bonus_time
                    .update_infallible(time_trial_bonus);
                asr::timer::set_variable_int("Time Trial Total Bonus", time_trial_bonus);
            }

            let time_trial_igt = addresses
                .time_trial_igt
                .deref::<f64>(game, &addresses.il2cpp_module, &addresses.game_assembly)
                .unwrap_or_default();
            let time_trial_igt_rounded = round_no_std_f64(time_trial_igt, 2);
            watchers
                .time_trial_igt
                .update_infallible(time_trial_igt_rounded);

            let time_trial_state_raw = addresses
                .time_trial_state
                .deref::<u32>(game, &addresses.il2cpp_module, &addresses.game_assembly)
                .unwrap_or_default();

            let time_trial_state = time_trial_state_int_to_enum(time_trial_state_raw);
            watchers
                .time_trial_state
                .update_infallible(time_trial_state);

            if settings.split_boss_phase {
                let boss_state = get_boss_state(&game, &addresses, &level_id);
                watchers.boss_state.update_infallible(boss_state);
                asr::timer::set_variable_int("Boss State", boss_state);
            }

            asr::timer::set_variable_float("Time Trial Timer", time_trial_igt);
            time_trial_state_print_var(time_trial_state);
        }
        TimerMode::TimeTrialMarathon => {
            if level_id != Stages::StageSelect && level_id != Stages::StageSelectPast {
                let stage_manager_state_int = addresses
                    .stage_manager_state
                    .deref::<u32>(game, &addresses.il2cpp_module, &addresses.game_assembly)
                    .unwrap_or_default();
                let stage_manager_state = get_stage_manager_state(stage_manager_state_int);
                watchers.stage_state.update_infallible(stage_manager_state);

                asr::timer::set_variable(
                    "Stage Manager State",
                    stage_state_to_string(stage_manager_state),
                );
            }

            let bonus_list_address_res = addresses.time_trial_bonus_list_pointer.deref::<u64>(
                game,
                &addresses.il2cpp_module,
                &addresses.game_assembly,
            );

            if let Ok(list_pointer) = bonus_list_address_res {
                let time_trial_bonus = calculate_time_bonus(game, list_pointer);
                watchers
                    .time_trial_bonus_time
                    .update_infallible(time_trial_bonus);
                asr::timer::set_variable_int("Time Trial Total Bonus", time_trial_bonus);
            }

            let time_trial_igt = addresses
                .time_trial_igt
                .deref::<f64>(game, &addresses.il2cpp_module, &addresses.game_assembly)
                .unwrap_or_default();

            let time_trial_igt_rounded = round_no_std_f64(time_trial_igt, 2);

            watchers
                .time_trial_igt
                .update_infallible(time_trial_igt_rounded);

            let time_trial_state_raw = addresses
                .time_trial_state
                .deref::<u32>(game, &addresses.il2cpp_module, &addresses.game_assembly)
                .unwrap_or_default();

            let time_trial_state = time_trial_state_int_to_enum(time_trial_state_raw);
            watchers
                .time_trial_state
                .update_infallible(time_trial_state);

            asr::timer::set_variable_float("Time Trial Timer", time_trial_igt);
            time_trial_state_print_var(time_trial_state);
        }
    }
}

fn round_no_std_f64(number: f64, digits: u32) -> f64 {
    let i = (number * (10_u32.pow(digits)) as f64) as i64;
    let frac = (number * (10_u32.pow(digits)) as f64) - (i as f64);

    if frac >= 0.5 {
        ((i + 1) as f64) / 100.
    } else {
        (i as f64) / 100.
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

fn get_player1_state(game: &Process, players_pointer: u64, addreses: &mut Memory) -> PlayerState {
    // all active "PlayerPacman"s are in an array, probably for 2p compatibility
    // so in the array obj, offset 0x20 is the PlayerPacman object we need, position 0
    let player_obj = game.read::<u64>(players_pointer + 0x20).unwrap_or_default();

    // if offset is still not init, abort memory read and set state as ASR not ready
    let player_state_offset = match addreses.player_state_offset {
        Some(offset) => offset,
        None => {
            addreses.refresh_player_state_offset(game);
            return PlayerState::ASROffsetNotReady;
        }
    };
    let player_state_int = game
        .read::<u32>(player_obj + player_state_offset as u64)
        .unwrap_or(100);

    match player_state_int {
        0 => PlayerState::None,
        1 => PlayerState::Control,
        2 => PlayerState::Damage,
        3 => PlayerState::FallDamage,
        4 => PlayerState::IcePoolDamage,
        5 => PlayerState::SnowBallDamage,
        6 => PlayerState::SinkDamage,
        7 => PlayerState::CutIn,
        8 => PlayerState::CutInGrap,
        9 => PlayerState::Gimmick,
        10 => PlayerState::SpaceJump,
        11 => PlayerState::SpaceJumpOut,
        12 => PlayerState::StageInit,
        13 => PlayerState::StageInitMaze,
        14 => PlayerState::StageInitSJ,
        15 => PlayerState::Dead,
        16 => PlayerState::Goal,
        17 => PlayerState::StageEnd,
        18 => PlayerState::Shooting,
        19 => PlayerState::Racing,
        100 => PlayerState::ASRReadError,
        _ => PlayerState::Unknown,
    }
}

fn player_state_to_string(player_state: PlayerState) -> &'static str {
    match player_state {
        PlayerState::None => "None",
        PlayerState::Control => "Control",
        PlayerState::Damage => "Damage",
        PlayerState::FallDamage => "Fall Damage",
        PlayerState::IcePoolDamage => "Ice Pool Damage",
        PlayerState::SnowBallDamage => "Snow Ball Damage",
        PlayerState::SinkDamage => "Sink Damage",
        PlayerState::CutIn => "Cut In",
        PlayerState::CutInGrap => "Cut In Grap",
        PlayerState::Gimmick => "Cut In Grap",
        PlayerState::SpaceJump => "Space Jump",
        PlayerState::SpaceJumpOut => "Space Jump Out",
        PlayerState::StageInit => "Stage Init",
        PlayerState::StageInitMaze => "Stage InitMaze",
        PlayerState::StageInitSJ => "Stage Init SJ",
        PlayerState::Dead => "Dead",
        PlayerState::Goal => "Goal",
        PlayerState::StageEnd => "StageEnd",
        PlayerState::Shooting => "Shooting",
        PlayerState::Racing => "Racing",
        PlayerState::ASRReadError => "ASR Memory Read Error",
        PlayerState::ASROffsetNotReady => "ASR Offset Not Ready",
        PlayerState::Unknown => "UNKNOWN",
    }
}

fn get_stage_manager_state(state: u32) -> StageState {
    match state {
        0 => StageState::None,
        1 => StageState::InitOnFade,
        2 => StageState::InitEndFade,
        3 => StageState::Playing,
        4 => StageState::Pause,
        5 => StageState::DebugPause,
        6 => StageState::Maze,
        7 => StageState::PacDead,
        8 => StageState::GameOver,
        9 => StageState::Goal,
        10 => StageState::Exit,
        _ => StageState::Unknown,
    }
}

fn stage_state_to_string(state: StageState) -> &'static str {
    match state {
        StageState::None => "None",
        StageState::InitOnFade => "Init On Fade",
        StageState::InitEndFade => "Init End Fade",
        StageState::Playing => "Playing",
        StageState::Pause => "Pause",
        StageState::DebugPause => "Debug Pause",
        StageState::Maze => "Maze",
        StageState::PacDead => "Pac Dead",
        StageState::GameOver => "Game Over",
        StageState::Goal => "Goal",
        StageState::Exit => "Exit",
        StageState::Unknown => "Unknown",
    }
}

fn get_boss_state(game: &Process, addresses: &Memory, level_id: &Stages) -> u32 {
    let mut boss_state = 0;

    if *level_id == Stages::Stage1_4
        || *level_id == Stages::Stage2_4
        || *level_id == Stages::Stage3_4
        || *level_id == Stages::Stage4_4
        || *level_id == Stages::Stage5_4
        || *level_id == Stages::Stage6_4
        || *level_id == Stages::Stage6_5
        || *level_id == Stages::Stage1_4Past
        || *level_id == Stages::Stage2_4Past
        || *level_id == Stages::Stage3_4Past
        || *level_id == Stages::Stage4_4Past
        || *level_id == Stages::Stage5_4Past
        || *level_id == Stages::Stage6_4Past
        || *level_id == Stages::Stage6_5
    {
        boss_state = addresses
            .boss_state
            .deref::<u32>(game, &addresses.il2cpp_module, &addresses.game_assembly)
            .unwrap_or_default();
    }

    boss_state
}

fn time_trial_state_int_to_enum(time_trial_state_raw: u32) -> TimeTrialState {
    match time_trial_state_raw {
        0 => TimeTrialState::None,
        1 => TimeTrialState::ReadyInit,
        2 => TimeTrialState::ReadyWait,
        3 => TimeTrialState::TA,
        4 => TimeTrialState::Pause,
        5 => TimeTrialState::End,
        _ => TimeTrialState::Unknown,
    }
}

fn time_trial_state_print_var(time_trial_state: TimeTrialState) {
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
