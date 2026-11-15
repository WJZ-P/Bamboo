//! 临时排查：多少个导出的模板带实体，都是谁。

use mc_bamboo_data::structures::ENTRIES;
use mc_bamboo_worldgen::structure::template::StructureTemplate;

#[test]
fn tmp_count_templates_with_entities() {
    let mut with_entities = Vec::new();
    for &(name, _, _) in ENTRIES {
        let template = StructureTemplate::load(name).unwrap();
        if !template.entities.is_empty() {
            with_entities.push((name, template.entities.len()));
        }
    }
    eprintln!("templates with entities: {}", with_entities.len());
    for (name, count) in &with_entities {
        eprintln!("  {name}: {count}");
    }
}
