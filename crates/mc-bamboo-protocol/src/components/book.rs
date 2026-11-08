//! 书与笔和成书共用的长度上限。
//!
//! 官方把同一组数写在两处：`WritableBookContent` / `WrittenBookContent` 的常量，
//! 以及 `ServerboundEditBookPacket` 的 `StreamCodec`（`list(100)`、`stringUtf8(1024)`、
//! `stringUtf8(32)`）。包解码和组件解码都引用这里，避免以后各改各的。
//!
//! Vanilla stores the same limits twice: the `WritableBookContent` /
//! `WrittenBookContent` constants, and `ServerboundEditBookPacket`'s `StreamCodec`
//! (`list(100)`, `stringUtf8(1024)`, `stringUtf8(32)`). Packet decoding and component
//! decoding both use this module so the two copies cannot drift.

/// 官方 `WritableBookContent.MAX_PAGES`，也是编辑包的 `ByteBufCodecs.list(100)`。
///
/// Official `WritableBookContent.MAX_PAGES`, and the edit packet's `ByteBufCodecs.list(100)`.
pub const MAX_PAGES: usize = 100;

/// 官方 `WritableBookContent.PAGE_EDIT_LENGTH`，也是编辑包每一页的 `stringUtf8(1024)`。
///
/// Official `WritableBookContent.PAGE_EDIT_LENGTH`, and each edit-packet page's `stringUtf8(1024)`.
pub const PAGE_EDIT_LENGTH: usize = 1024;

/// 官方 `WrittenBookContent.TITLE_MAX_LENGTH`，也是编辑包标题的 `stringUtf8(32)`。
///
/// Official `WrittenBookContent.TITLE_MAX_LENGTH`, and the edit-packet title's `stringUtf8(32)`.
pub const TITLE_MAX_LENGTH: usize = 32;
