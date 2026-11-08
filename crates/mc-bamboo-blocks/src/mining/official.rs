//! 官方 26.3 的挖掘进度逐位对照。
//!
//! `OFFICIAL` 的每一行由 scratchpad 里的 `MiningProgress.java` 用官方代码算出来：
//! `BlockBehaviour.getDestroyProgress` 和 `Player.getDestroySpeed`，没有状态效果。
//! 每行是“方块 物品 在地面 在水里 进度 bits 速度 bits”，bits 是 `Float.floatToIntBits` 的十进制值。
//! 这里把方块名和物品名换成 Bamboo 的常量，逐状态比较 `f32` 的位。

use mc_bamboo_data::{blocks, items};

use super::{Miner, destroy_progress, destroy_speed};

const OFFICIAL: &str = "\
stone air true false 1004172302 1065353216
stone air true true 984531775 1045220557
stone air false false 984531775 1045220557
stone air false true 965463908 1025758986
stone wooden_pickaxe true false 1026952033 1073741824
stone wooden_pickaxe true true 1007788724 1053609165
stone wooden_pickaxe false false 1007788724 1053609165
stone wooden_pickaxe false true 988349522 1034147594
stone stone_pickaxe true false 1035340641 1082130432
stone stone_pickaxe true true 1016177332 1061997773
stone stone_pickaxe false false 1016177332 1061997773
stone stone_pickaxe false true 996738130 1042536202
stone iron_pickaxe true false 1040746633 1086324736
stone iron_pickaxe true true 1020949518 1067030938
stone iron_pickaxe false false 1020949518 1067030938
stone iron_pickaxe false true 1001308991 1047904912
stone diamond_pickaxe true false 1043729249 1090519040
stone diamond_pickaxe true true 1024565940 1070386381
stone diamond_pickaxe false false 1024565940 1070386381
stone diamond_pickaxe false true 1005126738 1050924810
stone golden_pickaxe true false 1049135241 1094713344
stone golden_pickaxe true true 1029338126 1075419546
stone golden_pickaxe false false 1029338126 1075419546
stone golden_pickaxe false true 1009697599 1056293520
stone netherite_pickaxe true false 1045220557 1091567616
stone netherite_pickaxe true true 1025758987 1072064103
stone netherite_pickaxe false false 1025758986 1072064102
stone netherite_pickaxe false true 1006834287 1052266988
stone diamond_axe true false 1004172302 1065353216
stone diamond_axe true true 984531775 1045220557
stone diamond_axe false false 984531775 1045220557
stone diamond_axe false true 965463908 1025758986
stone diamond_shovel true false 1004172302 1065353216
stone diamond_shovel true true 984531775 1045220557
stone diamond_shovel false false 984531775 1045220557
stone diamond_shovel false true 965463908 1025758986
stone diamond_sword true false 1004172302 1065353216
stone diamond_sword true true 984531775 1045220557
stone diamond_sword false false 984531775 1045220557
stone diamond_sword false true 965463908 1025758986
stone shears true false 1004172302 1065353216
stone shears true true 984531775 1045220557
stone shears false false 984531775 1045220557
stone shears false true 965463908 1025758986
dirt air true false 1032358025 1065353216
dirt air true true 1012560910 1045220557
dirt air false false 1012560910 1045220557
dirt air false true 992920382 1025758986
dirt wooden_pickaxe true false 1032358025 1065353216
dirt wooden_pickaxe true true 1012560910 1045220557
dirt wooden_pickaxe false false 1012560910 1045220557
dirt wooden_pickaxe false true 992920382 1025758986
dirt stone_pickaxe true false 1032358025 1065353216
dirt stone_pickaxe true true 1012560910 1045220557
dirt stone_pickaxe false false 1012560910 1045220557
dirt stone_pickaxe false true 992920382 1025758986
dirt iron_pickaxe true false 1032358025 1065353216
dirt iron_pickaxe true true 1012560910 1045220557
dirt iron_pickaxe false false 1012560910 1045220557
dirt iron_pickaxe false true 992920382 1025758986
dirt diamond_pickaxe true false 1032358025 1065353216
dirt diamond_pickaxe true true 1012560910 1045220557
dirt diamond_pickaxe false false 1012560910 1045220557
dirt diamond_pickaxe false true 992920382 1025758986
dirt golden_pickaxe true false 1032358025 1065353216
dirt golden_pickaxe true true 1012560910 1045220557
dirt golden_pickaxe false false 1012560910 1045220557
dirt golden_pickaxe false true 992920382 1025758986
dirt netherite_pickaxe true false 1032358025 1065353216
dirt netherite_pickaxe true true 1012560910 1045220557
dirt netherite_pickaxe false false 1012560910 1045220557
dirt netherite_pickaxe false true 992920382 1025758986
dirt diamond_axe true false 1032358025 1065353216
dirt diamond_axe true true 1012560910 1045220557
dirt diamond_axe false false 1012560910 1045220557
dirt diamond_axe false true 992920382 1025758986
dirt diamond_shovel true false 1057523849 1090519040
dirt diamond_shovel true true 1037726734 1070386381
dirt diamond_shovel false false 1037726734 1070386381
dirt diamond_shovel false true 1018086206 1050924810
dirt diamond_sword true false 1032358025 1065353216
dirt diamond_sword true true 1012560910 1045220557
dirt diamond_sword false false 1012560910 1045220557
dirt diamond_sword false true 992920382 1025758986
dirt shears true false 1032358025 1065353216
dirt shears true true 1012560910 1045220557
dirt shears false false 1012560910 1045220557
dirt shears false true 992920382 1025758986
grass_block air true false 1029934649 1065353216
grass_block air true true 1010174816 1045220557
grass_block air false false 1010174816 1045220557
grass_block air false true 991011507 1025758986
grass_block wooden_pickaxe true false 1029934649 1065353216
grass_block wooden_pickaxe true true 1010174816 1045220557
grass_block wooden_pickaxe false false 1010174816 1045220557
grass_block wooden_pickaxe false true 991011507 1025758986
grass_block stone_pickaxe true false 1029934649 1065353216
grass_block stone_pickaxe true true 1010174816 1045220557
grass_block stone_pickaxe false false 1010174816 1045220557
grass_block stone_pickaxe false true 991011507 1025758986
grass_block iron_pickaxe true false 1029934649 1065353216
grass_block iron_pickaxe true true 1010174816 1045220557
grass_block iron_pickaxe false false 1010174816 1045220557
grass_block iron_pickaxe false true 991011507 1025758986
grass_block diamond_pickaxe true false 1029934649 1065353216
grass_block diamond_pickaxe true true 1010174816 1045220557
grass_block diamond_pickaxe false false 1010174816 1045220557
grass_block diamond_pickaxe false true 991011507 1025758986
grass_block golden_pickaxe true false 1029934649 1065353216
grass_block golden_pickaxe true true 1010174816 1045220557
grass_block golden_pickaxe false false 1010174816 1045220557
grass_block golden_pickaxe false true 991011507 1025758986
grass_block netherite_pickaxe true false 1029934649 1065353216
grass_block netherite_pickaxe true true 1010174816 1045220557
grass_block netherite_pickaxe false false 1010174816 1045220557
grass_block netherite_pickaxe false true 991011507 1025758986
grass_block diamond_axe true false 1029934649 1065353216
grass_block diamond_axe true true 1010174816 1045220557
grass_block diamond_axe false false 1010174816 1045220557
grass_block diamond_axe false true 991011507 1025758986
grass_block diamond_shovel true false 1055100473 1090519040
grass_block diamond_shovel true true 1035340640 1070386381
grass_block diamond_shovel false false 1035340640 1070386381
grass_block diamond_shovel false true 1016177331 1050924810
grass_block diamond_sword true false 1029934649 1065353216
grass_block diamond_sword true true 1010174816 1045220557
grass_block diamond_sword false false 1010174816 1045220557
grass_block diamond_sword false true 991011507 1025758986
grass_block shears true false 1029934649 1065353216
grass_block shears true true 1010174816 1045220557
grass_block shears false false 1010174816 1045220557
grass_block shears false true 991011507 1025758986
oak_log air true false 1015580809 1065353216
oak_log air true true 995783694 1045220557
oak_log air false false 995783694 1045220557
oak_log air false true 976143166 1025758986
oak_log wooden_pickaxe true false 1015580809 1065353216
oak_log wooden_pickaxe true true 995783694 1045220557
oak_log wooden_pickaxe false false 995783694 1045220557
oak_log wooden_pickaxe false true 976143166 1025758986
oak_log stone_pickaxe true false 1015580809 1065353216
oak_log stone_pickaxe true true 995783694 1045220557
oak_log stone_pickaxe false false 995783694 1045220557
oak_log stone_pickaxe false true 976143166 1025758986
oak_log iron_pickaxe true false 1015580809 1065353216
oak_log iron_pickaxe true true 995783694 1045220557
oak_log iron_pickaxe false false 995783694 1045220557
oak_log iron_pickaxe false true 976143166 1025758986
oak_log diamond_pickaxe true false 1015580809 1065353216
oak_log diamond_pickaxe true true 995783694 1045220557
oak_log diamond_pickaxe false false 995783694 1045220557
oak_log diamond_pickaxe false true 976143166 1025758986
oak_log golden_pickaxe true false 1015580809 1065353216
oak_log golden_pickaxe true true 995783694 1045220557
oak_log golden_pickaxe false false 995783694 1045220557
oak_log golden_pickaxe false true 976143166 1025758986
oak_log netherite_pickaxe true false 1015580809 1065353216
oak_log netherite_pickaxe true true 995783694 1045220557
oak_log netherite_pickaxe false false 995783694 1045220557
oak_log netherite_pickaxe false true 976143166 1025758986
oak_log diamond_axe true false 1040746633 1090519040
oak_log diamond_axe true true 1020949518 1070386381
oak_log diamond_axe false false 1020949518 1070386381
oak_log diamond_axe false true 1001308990 1050924810
oak_log diamond_shovel true false 1015580809 1065353216
oak_log diamond_shovel true true 995783694 1045220557
oak_log diamond_shovel false false 995783694 1045220557
oak_log diamond_shovel false true 976143166 1025758986
oak_log diamond_sword true false 1015580809 1065353216
oak_log diamond_sword true true 995783694 1045220557
oak_log diamond_sword false false 995783694 1045220557
oak_log diamond_sword false true 976143166 1025758986
oak_log shears true false 1015580809 1065353216
oak_log shears true true 995783694 1045220557
oak_log shears false false 995783694 1045220557
oak_log shears false true 976143166 1025758986
oak_planks air true false 1015580809 1065353216
oak_planks air true true 995783694 1045220557
oak_planks air false false 995783694 1045220557
oak_planks air false true 976143166 1025758986
oak_planks wooden_pickaxe true false 1015580809 1065353216
oak_planks wooden_pickaxe true true 995783694 1045220557
oak_planks wooden_pickaxe false false 995783694 1045220557
oak_planks wooden_pickaxe false true 976143166 1025758986
oak_planks stone_pickaxe true false 1015580809 1065353216
oak_planks stone_pickaxe true true 995783694 1045220557
oak_planks stone_pickaxe false false 995783694 1045220557
oak_planks stone_pickaxe false true 976143166 1025758986
oak_planks iron_pickaxe true false 1015580809 1065353216
oak_planks iron_pickaxe true true 995783694 1045220557
oak_planks iron_pickaxe false false 995783694 1045220557
oak_planks iron_pickaxe false true 976143166 1025758986
oak_planks diamond_pickaxe true false 1015580809 1065353216
oak_planks diamond_pickaxe true true 995783694 1045220557
oak_planks diamond_pickaxe false false 995783694 1045220557
oak_planks diamond_pickaxe false true 976143166 1025758986
oak_planks golden_pickaxe true false 1015580809 1065353216
oak_planks golden_pickaxe true true 995783694 1045220557
oak_planks golden_pickaxe false false 995783694 1045220557
oak_planks golden_pickaxe false true 976143166 1025758986
oak_planks netherite_pickaxe true false 1015580809 1065353216
oak_planks netherite_pickaxe true true 995783694 1045220557
oak_planks netherite_pickaxe false false 995783694 1045220557
oak_planks netherite_pickaxe false true 976143166 1025758986
oak_planks diamond_axe true false 1040746633 1090519040
oak_planks diamond_axe true true 1020949518 1070386381
oak_planks diamond_axe false false 1020949518 1070386381
oak_planks diamond_axe false true 1001308990 1050924810
oak_planks diamond_shovel true false 1015580809 1065353216
oak_planks diamond_shovel true true 995783694 1045220557
oak_planks diamond_shovel false false 995783694 1045220557
oak_planks diamond_shovel false true 976143166 1025758986
oak_planks diamond_sword true false 1015580809 1065353216
oak_planks diamond_sword true true 995783694 1045220557
oak_planks diamond_sword false false 995783694 1045220557
oak_planks diamond_sword false true 976143166 1025758986
oak_planks shears true false 1015580809 1065353216
oak_planks shears true true 995783694 1045220557
oak_planks shears false false 995783694 1045220557
oak_planks shears false true 976143166 1025758986
white_wool air true false 1026206379 1065353216
white_wool air true true 1007192201 1045220557
white_wool air false false 1007192201 1045220557
white_wool air false true 987395085 1025758986
white_wool wooden_pickaxe true false 1026206379 1065353216
white_wool wooden_pickaxe true true 1007192201 1045220557
white_wool wooden_pickaxe false false 1007192201 1045220557
white_wool wooden_pickaxe false true 987395085 1025758986
white_wool stone_pickaxe true false 1026206379 1065353216
white_wool stone_pickaxe true true 1007192201 1045220557
white_wool stone_pickaxe false false 1007192201 1045220557
white_wool stone_pickaxe false true 987395085 1025758986
white_wool iron_pickaxe true false 1026206379 1065353216
white_wool iron_pickaxe true true 1007192201 1045220557
white_wool iron_pickaxe false false 1007192201 1045220557
white_wool iron_pickaxe false true 987395085 1025758986
white_wool diamond_pickaxe true false 1026206379 1065353216
white_wool diamond_pickaxe true true 1007192201 1045220557
white_wool diamond_pickaxe false false 1007192201 1045220557
white_wool diamond_pickaxe false true 987395085 1025758986
white_wool golden_pickaxe true false 1026206379 1065353216
white_wool golden_pickaxe true true 1007192201 1045220557
white_wool golden_pickaxe false false 1007192201 1045220557
white_wool golden_pickaxe false true 987395085 1025758986
white_wool netherite_pickaxe true false 1026206379 1065353216
white_wool netherite_pickaxe true true 1007192201 1045220557
white_wool netherite_pickaxe false false 1007192201 1045220557
white_wool netherite_pickaxe false true 987395085 1025758986
white_wool diamond_axe true false 1026206379 1065353216
white_wool diamond_axe true true 1007192201 1045220557
white_wool diamond_axe false false 1007192201 1045220557
white_wool diamond_axe false true 987395085 1025758986
white_wool diamond_shovel true false 1026206379 1065353216
white_wool diamond_shovel true true 1007192201 1045220557
white_wool diamond_shovel false false 1007192201 1045220557
white_wool diamond_shovel false true 987395085 1025758986
white_wool diamond_sword true false 1026206379 1065353216
white_wool diamond_sword true true 1007192201 1045220557
white_wool diamond_sword false false 1007192201 1045220557
white_wool diamond_sword false true 987395085 1025758986
white_wool shears true false 1045779797 1084227584
white_wool shears true true 1026206379 1065353216
white_wool shears false false 1026206379 1065353216
white_wool shears false true 1007192201 1045220557
sand air true false 1032358025 1065353216
sand air true true 1012560910 1045220557
sand air false false 1012560910 1045220557
sand air false true 992920382 1025758986
sand wooden_pickaxe true false 1032358025 1065353216
sand wooden_pickaxe true true 1012560910 1045220557
sand wooden_pickaxe false false 1012560910 1045220557
sand wooden_pickaxe false true 992920382 1025758986
sand stone_pickaxe true false 1032358025 1065353216
sand stone_pickaxe true true 1012560910 1045220557
sand stone_pickaxe false false 1012560910 1045220557
sand stone_pickaxe false true 992920382 1025758986
sand iron_pickaxe true false 1032358025 1065353216
sand iron_pickaxe true true 1012560910 1045220557
sand iron_pickaxe false false 1012560910 1045220557
sand iron_pickaxe false true 992920382 1025758986
sand diamond_pickaxe true false 1032358025 1065353216
sand diamond_pickaxe true true 1012560910 1045220557
sand diamond_pickaxe false false 1012560910 1045220557
sand diamond_pickaxe false true 992920382 1025758986
sand golden_pickaxe true false 1032358025 1065353216
sand golden_pickaxe true true 1012560910 1045220557
sand golden_pickaxe false false 1012560910 1045220557
sand golden_pickaxe false true 992920382 1025758986
sand netherite_pickaxe true false 1032358025 1065353216
sand netherite_pickaxe true true 1012560910 1045220557
sand netherite_pickaxe false false 1012560910 1045220557
sand netherite_pickaxe false true 992920382 1025758986
sand diamond_axe true false 1032358025 1065353216
sand diamond_axe true true 1012560910 1045220557
sand diamond_axe false false 1012560910 1045220557
sand diamond_axe false true 992920382 1025758986
sand diamond_shovel true false 1057523849 1090519040
sand diamond_shovel true true 1037726734 1070386381
sand diamond_shovel false false 1037726734 1070386381
sand diamond_shovel false true 1018086206 1050924810
sand diamond_sword true false 1032358025 1065353216
sand diamond_sword true true 1012560910 1045220557
sand diamond_sword false false 1012560910 1045220557
sand diamond_sword false true 992920382 1025758986
sand shears true false 1032358025 1065353216
sand shears true true 1012560910 1045220557
sand shears false false 1012560910 1045220557
sand shears false true 992920382 1025758986
ice air true false 1032358025 1065353216
ice air true true 1012560910 1045220557
ice air false false 1012560910 1045220557
ice air false true 992920382 1025758986
ice wooden_pickaxe true false 1040746633 1073741824
ice wooden_pickaxe true true 1020949518 1053609165
ice wooden_pickaxe false false 1020949518 1053609165
ice wooden_pickaxe false true 1001308990 1034147594
ice stone_pickaxe true false 1049135241 1082130432
ice stone_pickaxe true true 1029338126 1061997773
ice stone_pickaxe false false 1029338126 1061997773
ice stone_pickaxe false true 1009697598 1042536202
ice iron_pickaxe true false 1053609165 1086324736
ice iron_pickaxe true true 1034147595 1067030938
ice iron_pickaxe false false 1034147595 1067030938
ice iron_pickaxe false true 1015222895 1047904912
ice diamond_pickaxe true false 1057523849 1090519040
ice diamond_pickaxe true true 1037726734 1070386381
ice diamond_pickaxe false false 1037726734 1070386381
ice diamond_pickaxe false true 1018086206 1050924810
ice golden_pickaxe true false 1061997773 1094713344
ice golden_pickaxe true true 1042536203 1075419546
ice golden_pickaxe false false 1042536203 1075419546
ice golden_pickaxe false true 1023611503 1056293520
ice netherite_pickaxe true false 1058642330 1091567616
ice netherite_pickaxe true true 1039516304 1072064103
ice netherite_pickaxe false false 1039516303 1072064102
ice netherite_pickaxe false true 1019517862 1052266988
ice diamond_axe true false 1032358025 1065353216
ice diamond_axe true true 1012560910 1045220557
ice diamond_axe false false 1012560910 1045220557
ice diamond_axe false true 992920382 1025758986
ice diamond_shovel true false 1032358025 1065353216
ice diamond_shovel true true 1012560910 1045220557
ice diamond_shovel false false 1012560910 1045220557
ice diamond_shovel false true 992920382 1025758986
ice diamond_sword true false 1032358025 1065353216
ice diamond_sword true true 1012560910 1045220557
ice diamond_sword false false 1012560910 1045220557
ice diamond_sword false true 992920382 1025758986
ice shears true false 1032358025 1065353216
ice shears true true 1012560910 1045220557
ice shears false false 1012560910 1045220557
ice shears false true 992920382 1025758986
cobweb air true false 992204554 1065353216
cobweb air true true 973279855 1045220557
cobweb air false false 973279855 1045220557
cobweb air false true 953267991 1025758986
cobweb wooden_pickaxe true false 992204554 1065353216
cobweb wooden_pickaxe true true 973279855 1045220557
cobweb wooden_pickaxe false false 973279855 1045220557
cobweb wooden_pickaxe false true 953267991 1025758986
cobweb stone_pickaxe true false 992204554 1065353216
cobweb stone_pickaxe true true 973279855 1045220557
cobweb stone_pickaxe false false 973279855 1045220557
cobweb stone_pickaxe false true 953267991 1025758986
cobweb iron_pickaxe true false 992204554 1065353216
cobweb iron_pickaxe true true 973279855 1045220557
cobweb iron_pickaxe false false 973279855 1045220557
cobweb iron_pickaxe false true 953267991 1025758986
cobweb diamond_pickaxe true false 992204554 1065353216
cobweb diamond_pickaxe true true 973279855 1045220557
cobweb diamond_pickaxe false false 973279855 1045220557
cobweb diamond_pickaxe false true 953267991 1025758986
cobweb golden_pickaxe true false 992204554 1065353216
cobweb golden_pickaxe true true 973279855 1045220557
cobweb golden_pickaxe false false 973279855 1045220557
cobweb golden_pickaxe false true 953267991 1025758986
cobweb netherite_pickaxe true false 992204554 1065353216
cobweb netherite_pickaxe true true 973279855 1045220557
cobweb netherite_pickaxe false false 973279855 1045220557
cobweb netherite_pickaxe false true 953267991 1025758986
cobweb diamond_axe true false 992204554 1065353216
cobweb diamond_axe true true 973279855 1045220557
cobweb diamond_axe false false 973279855 1045220557
cobweb diamond_axe false true 953267991 1025758986
cobweb diamond_shovel true false 992204554 1065353216
cobweb diamond_shovel true true 973279855 1045220557
cobweb diamond_shovel false false 973279855 1045220557
cobweb diamond_shovel false true 953267991 1025758986
cobweb diamond_sword true false 1040187392 1097859072
cobweb diamond_sword true true 1020054733 1077936128
cobweb diamond_sword false false 1020054733 1077936128
cobweb diamond_sword false true 1000593163 1058642330
cobweb shears true false 1040187392 1097859072
cobweb shears true true 1020054733 1077936128
cobweb shears false false 1020054733 1077936128
cobweb shears false true 1000593163 1058642330
obsidian air true false 961656599 1065353216
obsidian air true true 942130605 1045220557
obsidian air false false 942130605 1045220557
obsidian air false true 923154365 1025758986
obsidian wooden_pickaxe true false 970045207 1073741824
obsidian wooden_pickaxe true true 950519213 1053609165
obsidian wooden_pickaxe false false 950519213 1053609165
obsidian wooden_pickaxe false true 931542973 1034147594
obsidian stone_pickaxe true false 978433815 1082130432
obsidian stone_pickaxe true true 958907821 1061997773
obsidian stone_pickaxe false false 958907821 1061997773
obsidian stone_pickaxe false true 939931581 1042536202
obsidian iron_pickaxe true false 983386449 1086324736
obsidian iron_pickaxe true true 964405379 1067030938
obsidian iron_pickaxe false false 964405379 1067030938
obsidian iron_pickaxe false true 944329628 1047904912
obsidian diamond_pickaxe true false 1001308990 1090519040
obsidian diamond_pickaxe true true 982241125 1070386381
obsidian diamond_pickaxe false false 982241125 1070386381
obsidian diamond_pickaxe false true 962572859 1050924810
obsidian golden_pickaxe true false 991775057 1094713344
obsidian golden_pickaxe true true 972793987 1075419546
obsidian golden_pickaxe false false 972793987 1075419546
obsidian golden_pickaxe false true 952718236 1056293520
obsidian netherite_pickaxe true false 1002740646 1091567616
obsidian netherite_pickaxe true true 983386450 1072064103
obsidian netherite_pickaxe false false 983386449 1072064102
obsidian netherite_pickaxe false true 964405379 1052266988
obsidian diamond_axe true false 961656599 1065353216
obsidian diamond_axe true true 942130605 1045220557
obsidian diamond_axe false false 942130605 1045220557
obsidian diamond_axe false true 923154365 1025758986
obsidian diamond_shovel true false 961656599 1065353216
obsidian diamond_shovel true true 942130605 1045220557
obsidian diamond_shovel false false 942130605 1045220557
obsidian diamond_shovel false true 923154365 1025758986
obsidian diamond_sword true false 961656599 1065353216
obsidian diamond_sword true true 942130605 1045220557
obsidian diamond_sword false false 942130605 1045220557
obsidian diamond_sword false true 923154365 1025758986
obsidian shears true false 961656599 1065353216
obsidian shears true true 942130605 1045220557
obsidian shears false false 942130605 1045220557
obsidian shears false true 923154365 1025758986
diamond_ore air true false 995783694 1065353216
diamond_ore air true true 976143167 1045220557
diamond_ore air false false 976143167 1045220557
diamond_ore air false true 957075300 1025758986
diamond_ore wooden_pickaxe true false 1004172302 1073741824
diamond_ore wooden_pickaxe true true 984531775 1053609165
diamond_ore wooden_pickaxe false false 984531775 1053609165
diamond_ore wooden_pickaxe false true 965463908 1034147594
diamond_ore stone_pickaxe true false 1012560910 1082130432
diamond_ore stone_pickaxe true true 992920383 1061997773
diamond_ore stone_pickaxe false false 992920383 1061997773
diamond_ore stone_pickaxe false true 973852516 1042536202
diamond_ore iron_pickaxe true false 1032358025 1086324736
diamond_ore iron_pickaxe true true 1012560910 1067030938
diamond_ore iron_pickaxe false false 1012560910 1067030938
diamond_ore iron_pickaxe false true 992920383 1047904912
diamond_ore diamond_pickaxe true false 1035340641 1090519040
diamond_ore diamond_pickaxe true true 1016177332 1070386381
diamond_ore diamond_pickaxe false false 1016177332 1070386381
diamond_ore diamond_pickaxe false true 996738130 1050924810
diamond_ore golden_pickaxe true false 1025758986 1094713344
diamond_ore golden_pickaxe true true 1006834287 1075419546
diamond_ore golden_pickaxe false false 1006834287 1075419546
diamond_ore golden_pickaxe false true 986822424 1056293520
diamond_ore netherite_pickaxe true false 1036831949 1091567616
diamond_ore netherite_pickaxe true true 1017370379 1072064103
diamond_ore netherite_pickaxe false false 1017370378 1072064102
diamond_ore netherite_pickaxe false true 998445679 1052266988
diamond_ore diamond_axe true false 995783694 1065353216
diamond_ore diamond_axe true true 976143167 1045220557
diamond_ore diamond_axe false false 976143167 1045220557
diamond_ore diamond_axe false true 957075300 1025758986
diamond_ore diamond_shovel true false 995783694 1065353216
diamond_ore diamond_shovel true true 976143167 1045220557
diamond_ore diamond_shovel false false 976143167 1045220557
diamond_ore diamond_shovel false true 957075300 1025758986
diamond_ore diamond_sword true false 995783694 1065353216
diamond_ore diamond_sword true true 976143167 1045220557
diamond_ore diamond_sword false false 976143167 1045220557
diamond_ore diamond_sword false true 957075300 1025758986
diamond_ore shears true false 995783694 1065353216
diamond_ore shears true true 976143167 1045220557
diamond_ore shears false false 976143167 1045220557
diamond_ore shears false true 957075300 1025758986
deepslate air true false 995783694 1065353216
deepslate air true true 976143167 1045220557
deepslate air false false 976143167 1045220557
deepslate air false true 957075300 1025758986
deepslate wooden_pickaxe true false 1018563425 1073741824
deepslate wooden_pickaxe true true 999400116 1053609165
deepslate wooden_pickaxe false false 999400116 1053609165
deepslate wooden_pickaxe false true 979960914 1034147594
deepslate stone_pickaxe true false 1026952033 1082130432
deepslate stone_pickaxe true true 1007788724 1061997773
deepslate stone_pickaxe false false 1007788724 1061997773
deepslate stone_pickaxe false true 988349522 1042536202
deepslate iron_pickaxe true false 1032358025 1086324736
deepslate iron_pickaxe true true 1012560910 1067030938
deepslate iron_pickaxe false false 1012560910 1067030938
deepslate iron_pickaxe false true 992920383 1047904912
deepslate diamond_pickaxe true false 1035340641 1090519040
deepslate diamond_pickaxe true true 1016177332 1070386381
deepslate diamond_pickaxe false false 1016177332 1070386381
deepslate diamond_pickaxe false true 996738130 1050924810
deepslate golden_pickaxe true false 1040746633 1094713344
deepslate golden_pickaxe true true 1020949518 1075419546
deepslate golden_pickaxe false false 1020949518 1075419546
deepslate golden_pickaxe false true 1001308991 1056293520
deepslate netherite_pickaxe true false 1036831949 1091567616
deepslate netherite_pickaxe true true 1017370379 1072064103
deepslate netherite_pickaxe false false 1017370378 1072064102
deepslate netherite_pickaxe false true 998445679 1052266988
deepslate diamond_axe true false 995783694 1065353216
deepslate diamond_axe true true 976143167 1045220557
deepslate diamond_axe false false 976143167 1045220557
deepslate diamond_axe false true 957075300 1025758986
deepslate diamond_shovel true false 995783694 1065353216
deepslate diamond_shovel true true 976143167 1045220557
deepslate diamond_shovel false false 976143167 1045220557
deepslate diamond_shovel false true 957075300 1025758986
deepslate diamond_sword true false 995783694 1065353216
deepslate diamond_sword true true 976143167 1045220557
deepslate diamond_sword false false 976143167 1045220557
deepslate diamond_sword false true 957075300 1025758986
deepslate shears true false 995783694 1065353216
deepslate shears true true 976143167 1045220557
deepslate shears false false 976143167 1045220557
deepslate shears false true 957075300 1025758986
gold_ore air true false 995783694 1065353216
gold_ore air true true 976143167 1045220557
gold_ore air false false 976143167 1045220557
gold_ore air false true 957075300 1025758986
gold_ore wooden_pickaxe true false 1004172302 1073741824
gold_ore wooden_pickaxe true true 984531775 1053609165
gold_ore wooden_pickaxe false false 984531775 1053609165
gold_ore wooden_pickaxe false true 965463908 1034147594
gold_ore stone_pickaxe true false 1012560910 1082130432
gold_ore stone_pickaxe true true 992920383 1061997773
gold_ore stone_pickaxe false false 992920383 1061997773
gold_ore stone_pickaxe false true 973852516 1042536202
gold_ore iron_pickaxe true false 1032358025 1086324736
gold_ore iron_pickaxe true true 1012560910 1067030938
gold_ore iron_pickaxe false false 1012560910 1067030938
gold_ore iron_pickaxe false true 992920383 1047904912
gold_ore diamond_pickaxe true false 1035340641 1090519040
gold_ore diamond_pickaxe true true 1016177332 1070386381
gold_ore diamond_pickaxe false false 1016177332 1070386381
gold_ore diamond_pickaxe false true 996738130 1050924810
gold_ore golden_pickaxe true false 1025758986 1094713344
gold_ore golden_pickaxe true true 1006834287 1075419546
gold_ore golden_pickaxe false false 1006834287 1075419546
gold_ore golden_pickaxe false true 986822424 1056293520
gold_ore netherite_pickaxe true false 1036831949 1091567616
gold_ore netherite_pickaxe true true 1017370379 1072064103
gold_ore netherite_pickaxe false false 1017370378 1072064102
gold_ore netherite_pickaxe false true 998445679 1052266988
gold_ore diamond_axe true false 995783694 1065353216
gold_ore diamond_axe true true 976143167 1045220557
gold_ore diamond_axe false false 976143167 1045220557
gold_ore diamond_axe false true 957075300 1025758986
gold_ore diamond_shovel true false 995783694 1065353216
gold_ore diamond_shovel true true 976143167 1045220557
gold_ore diamond_shovel false false 976143167 1045220557
gold_ore diamond_shovel false true 957075300 1025758986
gold_ore diamond_sword true false 995783694 1065353216
gold_ore diamond_sword true true 976143167 1045220557
gold_ore diamond_sword false false 976143167 1045220557
gold_ore diamond_sword false true 957075300 1025758986
gold_ore shears true false 995783694 1065353216
gold_ore shears true true 976143167 1045220557
gold_ore shears false false 976143167 1045220557
gold_ore shears false true 957075300 1025758986
ancient_debris air true false 967754559 1065353216
ancient_debris air true true 948686693 1045220557
ancient_debris air false false 948686693 1045220557
ancient_debris air false true 929018427 1025758986
ancient_debris wooden_pickaxe true false 976143167 1073741824
ancient_debris wooden_pickaxe true true 957075301 1053609165
ancient_debris wooden_pickaxe false false 957075301 1053609165
ancient_debris wooden_pickaxe false true 937407035 1034147594
ancient_debris stone_pickaxe true false 984531775 1082130432
ancient_debris stone_pickaxe true true 965463909 1061997773
ancient_debris stone_pickaxe false false 965463909 1061997773
ancient_debris stone_pickaxe false true 945795643 1042536202
ancient_debris iron_pickaxe true false 990057071 1086324736
ancient_debris iron_pickaxe true true 970045208 1067030938
ancient_debris iron_pickaxe false false 970045208 1067030938
ancient_debris iron_pickaxe false true 950519213 1047904912
ancient_debris diamond_pickaxe true false 1007788724 1090519040
ancient_debris diamond_pickaxe true true 988349523 1070386381
ancient_debris diamond_pickaxe false false 988349523 1070386381
ancient_debris diamond_pickaxe false true 968518108 1050924810
ancient_debris golden_pickaxe true false 998445679 1094713344
ancient_debris golden_pickaxe true true 978433816 1075419546
ancient_debris golden_pickaxe false false 978433816 1075419546
ancient_debris golden_pickaxe false true 958907821 1056293520
ancient_debris netherite_pickaxe true false 1008981771 1091567616
ancient_debris netherite_pickaxe true true 990057071 1072064103
ancient_debris netherite_pickaxe false false 990057070 1072064102
ancient_debris netherite_pickaxe false true 970045207 1052266988
ancient_debris diamond_axe true false 967754559 1065353216
ancient_debris diamond_axe true true 948686693 1045220557
ancient_debris diamond_axe false false 948686693 1045220557
ancient_debris diamond_axe false true 929018427 1025758986
ancient_debris diamond_shovel true false 967754559 1065353216
ancient_debris diamond_shovel true true 948686693 1045220557
ancient_debris diamond_shovel false false 948686693 1045220557
ancient_debris diamond_shovel false true 929018427 1025758986
ancient_debris diamond_sword true false 967754559 1065353216
ancient_debris diamond_sword true true 948686693 1045220557
ancient_debris diamond_sword false false 948686693 1045220557
ancient_debris diamond_sword false true 929018427 1025758986
ancient_debris shears true false 967754559 1065353216
ancient_debris shears true true 948686693 1045220557
ancient_debris shears false false 948686693 1045220557
ancient_debris shears false true 929018427 1025758986
bedrock air true false 0 1065353216
bedrock air true true 0 1045220557
bedrock air false false 0 1045220557
bedrock air false true 0 1025758986
bedrock wooden_pickaxe true false 0 1065353216
bedrock wooden_pickaxe true true 0 1045220557
bedrock wooden_pickaxe false false 0 1045220557
bedrock wooden_pickaxe false true 0 1025758986
bedrock stone_pickaxe true false 0 1065353216
bedrock stone_pickaxe true true 0 1045220557
bedrock stone_pickaxe false false 0 1045220557
bedrock stone_pickaxe false true 0 1025758986
bedrock iron_pickaxe true false 0 1065353216
bedrock iron_pickaxe true true 0 1045220557
bedrock iron_pickaxe false false 0 1045220557
bedrock iron_pickaxe false true 0 1025758986
bedrock diamond_pickaxe true false 0 1065353216
bedrock diamond_pickaxe true true 0 1045220557
bedrock diamond_pickaxe false false 0 1045220557
bedrock diamond_pickaxe false true 0 1025758986
bedrock golden_pickaxe true false 0 1065353216
bedrock golden_pickaxe true true 0 1045220557
bedrock golden_pickaxe false false 0 1045220557
bedrock golden_pickaxe false true 0 1025758986
bedrock netherite_pickaxe true false 0 1065353216
bedrock netherite_pickaxe true true 0 1045220557
bedrock netherite_pickaxe false false 0 1045220557
bedrock netherite_pickaxe false true 0 1025758986
bedrock diamond_axe true false 0 1065353216
bedrock diamond_axe true true 0 1045220557
bedrock diamond_axe false false 0 1045220557
bedrock diamond_axe false true 0 1025758986
bedrock diamond_shovel true false 0 1065353216
bedrock diamond_shovel true true 0 1045220557
bedrock diamond_shovel false false 0 1045220557
bedrock diamond_shovel false true 0 1025758986
bedrock diamond_sword true false 0 1065353216
bedrock diamond_sword true true 0 1045220557
bedrock diamond_sword false false 0 1045220557
bedrock diamond_sword false true 0 1025758986
bedrock shears true false 0 1065353216
bedrock shears true true 0 1045220557
bedrock shears false false 0 1045220557
bedrock shears false true 0 1025758986
barrier air true false 0 1065353216
barrier air true true 0 1045220557
barrier air false false 0 1045220557
barrier air false true 0 1025758986
barrier wooden_pickaxe true false 0 1065353216
barrier wooden_pickaxe true true 0 1045220557
barrier wooden_pickaxe false false 0 1045220557
barrier wooden_pickaxe false true 0 1025758986
barrier stone_pickaxe true false 0 1065353216
barrier stone_pickaxe true true 0 1045220557
barrier stone_pickaxe false false 0 1045220557
barrier stone_pickaxe false true 0 1025758986
barrier iron_pickaxe true false 0 1065353216
barrier iron_pickaxe true true 0 1045220557
barrier iron_pickaxe false false 0 1045220557
barrier iron_pickaxe false true 0 1025758986
barrier diamond_pickaxe true false 0 1065353216
barrier diamond_pickaxe true true 0 1045220557
barrier diamond_pickaxe false false 0 1045220557
barrier diamond_pickaxe false true 0 1025758986
barrier golden_pickaxe true false 0 1065353216
barrier golden_pickaxe true true 0 1045220557
barrier golden_pickaxe false false 0 1045220557
barrier golden_pickaxe false true 0 1025758986
barrier netherite_pickaxe true false 0 1065353216
barrier netherite_pickaxe true true 0 1045220557
barrier netherite_pickaxe false false 0 1045220557
barrier netherite_pickaxe false true 0 1025758986
barrier diamond_axe true false 0 1065353216
barrier diamond_axe true true 0 1045220557
barrier diamond_axe false false 0 1045220557
barrier diamond_axe false true 0 1025758986
barrier diamond_shovel true false 0 1065353216
barrier diamond_shovel true true 0 1045220557
barrier diamond_shovel false false 0 1045220557
barrier diamond_shovel false true 0 1025758986
barrier diamond_sword true false 0 1065353216
barrier diamond_sword true true 0 1045220557
barrier diamond_sword false false 0 1045220557
barrier diamond_sword false true 0 1025758986
barrier shears true false 0 1065353216
barrier shears true true 0 1045220557
barrier shears false false 0 1045220557
barrier shears false true 0 1025758986
";

fn state_of(name: &str) -> u32 {
    match name {
        "stone" => blocks::STONE,
        "dirt" => blocks::DIRT,
        "grass_block" => blocks::GRASS_BLOCK,
        "oak_log" => blocks::OAK_LOG,
        "oak_planks" => blocks::OAK_PLANKS,
        "white_wool" => blocks::WHITE_WOOL,
        "sand" => blocks::SAND,
        "ice" => blocks::ICE,
        "cobweb" => blocks::COBWEB,
        "obsidian" => blocks::OBSIDIAN,
        "diamond_ore" => blocks::DIAMOND_ORE,
        "deepslate" => blocks::DEEPSLATE,
        "gold_ore" => blocks::GOLD_ORE,
        "ancient_debris" => blocks::ANCIENT_DEBRIS,
        "bedrock" => blocks::BEDROCK,
        "barrier" => blocks::BARRIER,
        other => panic!("the official table names an unknown block: {other}"),
    }
}

fn item_of(name: &str) -> i32 {
    match name {
        "air" => items::AIR,
        "wooden_pickaxe" => items::WOODEN_PICKAXE,
        "stone_pickaxe" => items::STONE_PICKAXE,
        "iron_pickaxe" => items::IRON_PICKAXE,
        "diamond_pickaxe" => items::DIAMOND_PICKAXE,
        "golden_pickaxe" => items::GOLDEN_PICKAXE,
        "netherite_pickaxe" => items::NETHERITE_PICKAXE,
        "diamond_axe" => items::DIAMOND_AXE,
        "diamond_shovel" => items::DIAMOND_SHOVEL,
        "diamond_sword" => items::DIAMOND_SWORD,
        "shears" => items::SHEARS,
        other => panic!("the official table names an unknown item: {other}"),
    }
}

#[test]
fn official_destroy_progress_matches_bit_for_bit() {
    let mut rows = 0;
    for (index, line) in OFFICIAL.lines().enumerate() {
        let mut fields = line.split_whitespace();
        let block = fields.next().expect("block");
        let item = fields.next().expect("item");
        let on_ground = fields.next().expect("onGround") == "true";
        let eye_in_water = fields.next().expect("inWater") == "true";
        let progress_bits: u32 = fields
            .next()
            .expect("progress")
            .parse()
            .expect("progress bits");
        let speed_bits: u32 = fields.next().expect("speed").parse().expect("speed bits");
        assert!(
            fields.next().is_none(),
            "line {} has extra fields",
            index + 1
        );

        let state = state_of(block);
        let miner = Miner {
            main_hand: item_of(item),
            on_ground,
            eye_in_water,
        };
        let progress = destroy_progress(state, miner);
        assert_eq!(
            progress.to_bits(),
            progress_bits,
            "line {}: {block} {item} ground={on_ground} water={eye_in_water}",
            index + 1
        );
        let speed = destroy_speed(state, miner);
        assert_eq!(
            speed.to_bits(),
            speed_bits,
            "line {}: {block} {item} ground={on_ground} water={eye_in_water}",
            index + 1
        );
        rows += 1;
    }
    assert_eq!(rows, 704);
}
