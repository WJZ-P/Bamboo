//! 标题、副标题、动作栏和显示时间。
//!
//! 五种 clientbound 包：`ClientboundClearTitlesPacket`（`resetTimes` 一个布尔）、
//! `ClientboundSetTitleTextPacket` / `ClientboundSetSubtitleTextPacket` /
//! `ClientboundSetActionBarTextPacket`（各一个 NBT 文本组件），
//! 以及 `ClientboundSetTitlesAnimationPacket`（三个大端 int）。
//!
//! Titles, subtitles, action bars and their timing. The three text packets each
//! carry one NBT component; the animation packet carries three big-endian ints.

use mc_bamboo_data::packets::play::clientbound;

use crate::{
    codec::{write_bool, write_i32},
    packet::ClientboundPacket,
    text::Component,
};

/// `ClientboundClearTitlesPacket`。`reset_times` 为真时连显示时间一起重置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClearTitles {
    pub reset_times: bool,
}

impl ClientboundPacket for ClearTitles {
    const ID: i32 = clientbound::CLEAR_TITLES;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_bool(out, self.reset_times);
    }
}

/// `ClientboundSetTitleTextPacket`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetTitleText<'a> {
    pub text: &'a Component,
}

impl ClientboundPacket for SetTitleText<'_> {
    const ID: i32 = clientbound::SET_TITLE_TEXT;

    fn write_body(&self, out: &mut Vec<u8>) {
        self.text.write(out);
    }
}

/// `ClientboundSetSubtitleTextPacket`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetSubtitleText<'a> {
    pub text: &'a Component,
}

impl ClientboundPacket for SetSubtitleText<'_> {
    const ID: i32 = clientbound::SET_SUBTITLE_TEXT;

    fn write_body(&self, out: &mut Vec<u8>) {
        self.text.write(out);
    }
}

/// `ClientboundSetActionBarTextPacket`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetActionBarText<'a> {
    pub text: &'a Component,
}

impl ClientboundPacket for SetActionBarText<'_> {
    const ID: i32 = clientbound::SET_ACTION_BAR_TEXT;

    fn write_body(&self, out: &mut Vec<u8>) {
        self.text.write(out);
    }
}

/// `ClientboundSetTitlesAnimationPacket`：淡入、停留、淡出，单位是游戏刻。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetTitlesAnimation {
    pub fade_in: i32,
    pub stay: i32,
    pub fade_out: i32,
}

impl ClientboundPacket for SetTitlesAnimation {
    const ID: i32 = clientbound::SET_TITLES_ANIMATION;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_i32(out, self.fade_in);
        write_i32(out, self.stay);
        write_i32(out, self.fade_out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::test_util::{body, frame, hex, string_entry};

    #[test]
    fn clear_titles_writes_one_boolean() {
        assert_eq!(body(&ClearTitles { reset_times: false }), hex("00"));
        assert_eq!(body(&ClearTitles { reset_times: true }), hex("01"));
        assert_eq!(frame(&ClearTitles { reset_times: true }), hex("02 0e 01"));
    }

    #[test]
    fn title_text_is_one_nbt_component() {
        let component = Component::text("hi");
        let packet = SetTitleText { text: &component };
        // 裸的 TAG_String：`08`、两字节长度、`hi`。
        assert_eq!(body(&packet), hex("08 00 02 68 69"));
        assert_eq!(frame(&packet), hex("06 74 08 00 02 68 69"));
    }

    #[test]
    fn subtitle_and_action_bar_share_the_component_encoding() {
        let component = Component::text("hi");
        assert_eq!(
            body(&SetSubtitleText { text: &component }),
            body(&SetActionBarText { text: &component })
        );
        let styled = Component::text("hi").color(crate::text::Color::Red);
        let mut expected = vec![0x0a];
        expected.extend(string_entry("text", "hi"));
        expected.extend(string_entry("color", "red"));
        expected.push(0);
        assert_eq!(body(&SetActionBarText { text: &styled }), expected);
    }

    #[test]
    fn animation_writes_three_big_endian_ints() {
        let packet = SetTitlesAnimation {
            fade_in: 10,
            stay: 70,
            fade_out: 20,
        };
        assert_eq!(body(&packet), hex("00 00 00 0a 00 00 00 46 00 00 00 14"));
    }

    #[test]
    fn ids_match_the_official_registry() {
        assert_eq!(ClearTitles::ID, 14);
        assert_eq!(SetActionBarText::ID, 89);
        assert_eq!(SetSubtitleText::ID, 114);
        assert_eq!(SetTitleText::ID, 116);
        assert_eq!(SetTitlesAnimation::ID, 117);
    }
}
