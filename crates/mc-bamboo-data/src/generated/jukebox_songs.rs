// 这个文件由 mc-bamboo-vanilla 生成，不要手动修改。

/// 一张唱片：长度（秒）和比较器输出。下标是 `minecraft:jukebox_song` 的注册表 ID。
pub struct JukeboxSong {
    pub length_in_seconds: f32,
    pub comparator_output: i32,
}

/// `JukeboxSongs.bootstrap`，按注册表 ID 排列，不是按注册函数的调用顺序。
pub static SONGS: [JukeboxSong; 22] = [
    JukeboxSong { length_in_seconds: 71.0, comparator_output: 11 },
    JukeboxSong { length_in_seconds: 178.0, comparator_output: 1 },
    JukeboxSong { length_in_seconds: 178.0, comparator_output: 15 },
    JukeboxSong { length_in_seconds: 345.0, comparator_output: 3 },
    JukeboxSong { length_in_seconds: 234.0, comparator_output: 8 },
    JukeboxSong { length_in_seconds: 185.0, comparator_output: 2 },
    JukeboxSong { length_in_seconds: 185.0, comparator_output: 4 },
    JukeboxSong { length_in_seconds: 176.0, comparator_output: 12 },
    JukeboxSong { length_in_seconds: 73.0, comparator_output: 11 },
    JukeboxSong { length_in_seconds: 174.0, comparator_output: 5 },
    JukeboxSong { length_in_seconds: 134.0, comparator_output: 9 },
    JukeboxSong { length_in_seconds: 197.0, comparator_output: 6 },
    JukeboxSong { length_in_seconds: 96.0, comparator_output: 7 },
    JukeboxSong { length_in_seconds: 195.0, comparator_output: 14 },
    JukeboxSong { length_in_seconds: 149.0, comparator_output: 13 },
    JukeboxSong { length_in_seconds: 299.0, comparator_output: 13 },
    JukeboxSong { length_in_seconds: 218.0, comparator_output: 14 },
    JukeboxSong { length_in_seconds: 150.0, comparator_output: 8 },
    JukeboxSong { length_in_seconds: 188.0, comparator_output: 9 },
    JukeboxSong { length_in_seconds: 175.0, comparator_output: 10 },
    JukeboxSong { length_in_seconds: 238.0, comparator_output: 12 },
    JukeboxSong { length_in_seconds: 251.0, comparator_output: 10 },
];

/// 按注册表 ID 取一首歌。ID 不在表里时是 `None`。
pub fn get(id: i32) -> Option<&'static JukeboxSong> {
    usize::try_from(id).ok().and_then(|id| SONGS.get(id))
}
