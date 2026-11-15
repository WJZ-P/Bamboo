use mc_bamboo_worldgen::{Worldgen, biome_manager::biome_id, chunk::Generator};

#[test]
fn tmp_find_biomes() {
    let worldgen = Worldgen::load().unwrap();
    let generator = Generator::new(&worldgen, 0).unwrap();
    let want = biome_id("minecraft:lush_caves");
    let mut found = 0;
    for r in 0..2000 {
        let d = r * 3;
        for (x, z) in [(d, d), (-d, d), (d, -d), (-d, -d), (d, 0), (0, d)] {
            let Ok(sections) = generator.biomes(x, z) else {
                continue;
            };
            if sections.iter().flatten().any(|&id| id == want) {
                println!("LUSH {x} {z}");
                found += 1;
                if found >= 40 {
                    return;
                }
            }
        }
    }
}
