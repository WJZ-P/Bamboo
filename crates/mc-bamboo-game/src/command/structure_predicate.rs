//! 在已交付区块的结构起点及结构件中查位置。
//!
//! Look up positions in structure starts and pieces of delivered chunks.

use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_nbt::Tag;
use mc_bamboo_world::store::ChunkStore;
use mc_bamboo_worldgen::structure::save::load_start;

pub(super) fn matches(chunks: &ChunkStore, pos: BlockPos, structures: &[String]) -> bool {
    let references = structures_field(chunks, pos.chunk(), "References");
    structures.iter().any(|structure| {
        let Some(Tag::LongArray(references)) = field(references, structure) else {
            return false;
        };
        references.iter().any(|&packed| {
            let (x, z) = crate::worldgen_structures::unpack_chunk_pos(packed);
            let starts = structures_field(chunks, ChunkPos::new(x, z), "starts");
            field(starts, structure)
                .and_then(load_start)
                .is_some_and(|start| {
                    start.structure == *structure
                        && start
                            .pieces
                            .iter()
                            .any(|piece| piece.bb.contains(pos.x, pos.y, pos.z))
                })
        })
    })
}

fn structures_field<'a>(chunks: &'a ChunkStore, chunk: ChunkPos, key: &str) -> Option<&'a Tag> {
    if !chunks.is_loaded(chunk) {
        return None;
    }
    field(chunks.chunk(chunk).archive().structures.as_ref(), key)
}

fn field<'a>(tag: Option<&'a Tag>, key: &str) -> Option<&'a Tag> {
    let Tag::Compound(entries) = tag? else {
        return None;
    };
    entries
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use mc_bamboo_world::column::ChunkColumn;
    use mc_bamboo_worldgen::structure::{
        save::{SavePiece, SaveStart, structures_tag},
        start::BoundingBox,
    };

    use super::*;

    #[test]
    fn follows_delivered_references_and_checks_individual_pieces() {
        let id = "minecraft:stronghold".to_owned();
        let target = BlockPos::new(16, 5, 0);
        let mut chunks = ChunkStore::loaded();
        let pieces = [16, 19].map(|x| SavePiece {
            id: "minecraft:shs".into(),
            bb: BoundingBox {
                min_x: x,
                min_y: 5,
                min_z: 0,
                max_x: x,
                max_y: 5,
                max_z: 0,
            },
            orientation: -1,
            gen_depth: 0,
            extras: Vec::new(),
        });
        let start = SaveStart {
            structure: id.clone(),
            chunk_x: 0,
            chunk_z: 0,
            references: 0,
            children: pieces.to_vec(),
        };
        let mut install = |pos, starts, refs| {
            let mut column = ChunkColumn::from_template();
            let mut archive = column.chunk().archive().clone();
            archive.structures = Some(structures_tag(&starts, &refs));
            column.chunk_mut().set_archive(archive);
            chunks.load(pos, Box::new(column));
        };
        install(
            ChunkPos::new(0, 0),
            BTreeMap::from([(id.clone(), start)]),
            BTreeMap::new(),
        );
        install(
            target.chunk(),
            BTreeMap::new(),
            BTreeMap::from([(id.clone(), vec![0])]),
        );
        assert!(matches(&chunks, target, std::slice::from_ref(&id)));
        assert!(!matches(&chunks, target.offset(1, 0, 0), &[id]));
    }
}
