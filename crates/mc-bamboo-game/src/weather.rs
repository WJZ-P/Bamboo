//! 官方 `ServerLevel.advanceWeatherCycle`：每刻倒计时、自然切换雨和雷、渐变强度和广播。
//!
//! 权威状态在 [`Global`](crate::global::Global) 的 [`Weather`](crate::world_state::Weather) 上。
//! 方块逻辑和进服包都读同一份状态；`/weather` 只改状态，包由这里的循环按官方条件发。

use mc_bamboo_blocks::random::RandomSource;
use mc_bamboo_data::gamerules;
use mc_bamboo_protocol::packet::play::GameEvent;

use crate::{global::Global, links::encode_frames, region::Region};

/// `ServerLevel` 的四个 `IntProvider`（`UniformInt.sample`）。
const RAIN_DELAY: (i32, i32) = (12_000, 180_000);
const RAIN_DURATION: (i32, i32) = (12_000, 24_000);
const THUNDER_DELAY: (i32, i32) = (12_000, 180_000);
const THUNDER_DURATION: (i32, i32) = (3_600, 15_600);
/// 每刻渐变强度走这么多。
const LEVEL_STEP: f32 = 0.01;

/// `ServerLevel.tick` 的天气阶段：在 `tickTime` 之前，只在 `advance_weather` 为真时推进倒计时。
///
/// 广播顺序照 `advanceWeatherCycle`：先两个强度变化，再开始/停止下雨（变化时无条件再发两个强度包）。
/// 返回要广播的帧，由世界线程统一发出去。
pub(crate) fn advance(global: &mut Global, region: &Region) -> Vec<u8> {
    let was_raining = global.state().weather.is_raining();
    let advance = global.rules().bool(gamerules::ADVANCE_WEATHER);
    if advance {
        let random = region.random.source();
        let mut state = global.state().weather;
        if state.clear_time > 0 {
            state.clear_time -= 1;
            state.thunder_time = i32::from(!state.thundering);
            state.rain_time = i32::from(!state.raining);
            state.thundering = false;
            state.raining = false;
        } else {
            if state.thunder_time > 0 {
                state.thunder_time -= 1;
                if state.thunder_time == 0 {
                    state.thundering = !state.thundering;
                }
            } else {
                let (min, max) = if state.thundering {
                    THUNDER_DURATION
                } else {
                    THUNDER_DELAY
                };
                state.thunder_time = RandomSource::next_int_between_inclusive(random, min, max);
            }
            if state.rain_time > 0 {
                state.rain_time -= 1;
                if state.rain_time == 0 {
                    state.raining = !state.raining;
                }
            } else {
                let (min, max) = if state.raining {
                    RAIN_DURATION
                } else {
                    RAIN_DELAY
                };
                state.rain_time = RandomSource::next_int_between_inclusive(random, min, max);
            }
        }
        state.o_thunder_level = state.thunder_level;
        state.thunder_level = step(state.thunder_level, state.thundering);
        state.o_rain_level = state.rain_level;
        state.rain_level = step(state.rain_level, state.raining);
        global.state_mut().weather = state;
    }
    let state = global.state().weather;
    let mut events = Vec::new();
    if state.o_rain_level != state.rain_level {
        events.push(GameEvent {
            event: GameEvent::RAIN_LEVEL_CHANGE,
            param: state.rain_level,
        });
    }
    if state.o_thunder_level != state.thunder_level {
        events.push(GameEvent {
            event: GameEvent::THUNDER_LEVEL_CHANGE,
            param: state.thunder_level,
        });
    }
    if was_raining != state.is_raining() {
        events.push(GameEvent {
            event: if was_raining {
                GameEvent::STOP_RAINING
            } else {
                GameEvent::START_RAINING
            },
            param: 0.0,
        });
        events.push(GameEvent {
            event: GameEvent::RAIN_LEVEL_CHANGE,
            param: state.rain_level,
        });
        events.push(GameEvent {
            event: GameEvent::THUNDER_LEVEL_CHANGE,
            param: state.thunder_level,
        });
    }
    let mut frames = Vec::new();
    for event in &events {
        frames.extend(encode_frames(event));
    }
    frames
}

/// 权威状态变了之后把快照复制给方块逻辑。
pub(crate) fn sync(global: &Global, region: &mut Region) {
    region.block_weather = global.state().weather.snapshot();
}

fn step(level: f32, rising: bool) -> f32 {
    if rising {
        (level + LEVEL_STEP).min(1.0)
    } else {
        (level - LEVEL_STEP).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use mc_bamboo_blocks::LevelRandom;

    use super::*;
    use crate::{World, WorldConfig};

    fn world() -> World {
        let (_sender, receiver) = mpsc::channel();
        let mut world = World::new(
            WorldConfig {
                view_distance: 2,
                max_players: 20,
                online_mode: false,
                enforce_secure_profile: false,
                admin: Default::default(),
            },
            receiver,
        );
        world.region.random = LevelRandom::new(0);
        world
    }

    #[test]
    fn a_rain_command_counts_down_and_stops() {
        let mut world = world();
        let state = &mut world.global.state_mut().weather;
        state.raining = true;
        state.rain_time = 2;
        state.thunder_time = 2;
        state.rain_level = 1.0;
        state.o_rain_level = 1.0;
        state.thunder_level = 0.0;
        state.o_thunder_level = 0.0;
        world.region.block_weather = state.snapshot();

        // 饱和的雨强在 `clear_time` 的分支里保持不变，但每一刻仍可能因为
        // `o_rain_level != rain_level` 发强度包；这里只关心状态倒计时。
        for _ in 0..2 {
            let _ = advance(&mut world.global, &world.region);
            sync(&world.global, &mut world.region);
        }
        assert_eq!(world.global.state().weather.rain_time, 0);
        assert!(!world.global.state().weather.raining);
        // 官方在 WeatherData 翻转后仍要 80 刻渐变才发 STOP_RAINING。
        for _ in 0..20 {
            let _ = advance(&mut world.global, &world.region);
        }
        assert!(world.global.state().weather.is_raining());
        for _ in 0..60 {
            let _ = advance(&mut world.global, &world.region);
        }
        assert!(!world.global.state().weather.is_raining());
        assert!(world.global.state().weather.rain_level <= 0.2);
    }

    #[test]
    fn advance_weather_false_freezes_the_cycle() {
        let mut world = world();
        world
            .global
            .rules_mut()
            .assign(gamerules::ADVANCE_WEATHER, "false");
        let state = world.global.state().weather;
        for _ in 0..10 {
            let _ = advance(&mut world.global, &world.region);
        }
        assert_eq!(world.global.state().weather, state);
    }

    #[test]
    fn a_fresh_world_starts_a_clear_delay() {
        let mut world = world();
        let _ = advance(&mut world.global, &world.region);
        let state = world.global.state().weather;
        assert!(!state.raining);
        assert!(!state.thundering);
        assert_eq!(state.clear_time, 0);
        assert!(state.rain_time > 0);
        assert!(state.thunder_time > 0);
    }
}
