// 这个文件由 mc-bamboo-vanilla 生成，不要手动修改。

use crate::{CookingKind, FireworkShape, Ingredient, PotionIngredient, Recipe, RecipeKind, Stack};

/// 药水注册表的条目名，下标就是 ID。
pub static POTIONS: &[&str] = &["minecraft:water", "minecraft:mundane", "minecraft:thick", "minecraft:awkward", "minecraft:night_vision", "minecraft:long_night_vision", "minecraft:invisibility", "minecraft:long_invisibility", "minecraft:leaping", "minecraft:long_leaping", "minecraft:strong_leaping", "minecraft:fire_resistance", "minecraft:long_fire_resistance", "minecraft:swiftness", "minecraft:long_swiftness", "minecraft:strong_swiftness", "minecraft:slowness", "minecraft:long_slowness", "minecraft:strong_slowness", "minecraft:turtle_master", "minecraft:long_turtle_master", "minecraft:strong_turtle_master", "minecraft:water_breathing", "minecraft:long_water_breathing", "minecraft:healing", "minecraft:strong_healing", "minecraft:harming", "minecraft:strong_harming", "minecraft:poison", "minecraft:long_poison", "minecraft:strong_poison", "minecraft:regeneration", "minecraft:long_regeneration", "minecraft:strong_regeneration", "minecraft:strength", "minecraft:long_strength", "minecraft:strong_strength", "minecraft:weakness", "minecraft:long_weakness", "minecraft:luck", "minecraft:slow_falling", "minecraft:long_slow_falling", "minecraft:wind_charged", "minecraft:weaving", "minecraft:oozing", "minecraft:infested"];

/// 默认物品栈带 `max_damage` 和 `damage` 的物品，升序。
pub static DAMAGEABLE: &[i32] = &[971, 972, 974, 1001, 1004, 1005, 1008, 1025, 1026, 1027, 1028, 1029, 1030, 1031, 1032, 1033, 1034, 1035, 1036, 1037, 1038, 1039, 1040, 1041, 1042, 1043, 1044, 1045, 1046, 1047, 1048, 1049, 1050, 1051, 1052, 1053, 1054, 1055, 1056, 1057, 1058, 1059, 1068, 1069, 1070, 1071, 1072, 1073, 1074, 1075, 1076, 1077, 1078, 1079, 1080, 1081, 1082, 1083, 1084, 1085, 1086, 1087, 1088, 1089, 1090, 1091, 1092, 1093, 1094, 1095, 1186, 1255, 1374, 1446, 1447, 1448, 1449, 1450, 1451, 1452, 1453, 1483, 1491, 1578];

/// 默认物品栈带 `dye` 组件的物品，升序。
pub static DYED: &[i32] = &[1199, 1200, 1201, 1202, 1203, 1204, 1205, 1206, 1207, 1208, 1209, 1210, 1211, 1212, 1213, 1214];

const ITEM_LIST_0: &[i32] = &[67];
const ITEM_LIST_1: &[i32] = &[404];
const ITEM_LIST_2: &[i32] = &[983];
const ITEM_LIST_3: &[i32] = &[1060];
const ITEM_LIST_4: &[i32] = &[473];
const ITEM_LIST_5: &[i32] = &[182];
const ITEM_LIST_6: &[i32] = &[167, 182, 194, 207];
const ITEM_LIST_7: &[i32] = &[167];
const ITEM_LIST_8: &[i32] = &[1018];
const ITEM_LIST_9: &[i32] = &[825];
const ITEM_LIST_10: &[i32] = &[1016];
const ITEM_LIST_11: &[i32] = &[4];
const ITEM_LIST_12: &[i32] = &[62];
const ITEM_LIST_13: &[i32] = &[6];
const ITEM_LIST_14: &[i32] = &[119];
const ITEM_LIST_15: &[i32] = &[356];
const ITEM_LIST_16: &[i32] = &[1096];
const ITEM_LIST_17: &[i32] = &[1063];
const ITEM_LIST_18: &[i32] = &[1379];
const ITEM_LIST_19: &[i32] = &[340];
const ITEM_LIST_20: &[i32] = &[73];
const ITEM_LIST_21: &[i32] = &[995];
const ITEM_LIST_22: &[i32] = &[202];
const ITEM_LIST_23: &[i32] = &[351];
const ITEM_LIST_24: &[i32] = &[76];
const ITEM_LIST_25: &[i32] = &[177, 202];
const ITEM_LIST_26: &[i32] = &[63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75];
const ITEM_LIST_27: &[i32] = &[341, 342, 343, 344, 345, 346, 347, 348, 349, 350, 351, 353, 354];
const ITEM_LIST_28: &[i32] = &[231];
const ITEM_LIST_29: &[i32] = &[1391];
const ITEM_LIST_30: &[i32] = &[394];
const ITEM_LIST_31: &[i32] = &[1530];
const ITEM_LIST_32: &[i32] = &[1006];
const ITEM_LIST_33: &[i32] = &[1438];
const ITEM_LIST_34: &[i32] = &[65];
const ITEM_LIST_35: &[i32] = &[979];
const ITEM_LIST_36: &[i32] = &[180];
const ITEM_LIST_37: &[i32] = &[165, 180, 192, 205];
const ITEM_LIST_38: &[i32] = &[165];
const ITEM_LIST_39: &[i32] = &[265];
const ITEM_LIST_40: &[i32] = &[1432];
const ITEM_LIST_41: &[i32] = &[1153, 1154, 1155, 1156, 1157, 1158, 1159, 1160, 1161, 1162, 1163, 1164, 1165, 1166, 1167, 1168, 1169];
const ITEM_LIST_42: &[i32] = &[1214];
const ITEM_LIST_43: &[i32] = &[1550];
const ITEM_LIST_44: &[i32] = &[88];
const ITEM_LIST_45: &[i32] = &[92];
const ITEM_LIST_46: &[i32] = &[704];
const ITEM_LIST_47: &[i32] = &[297];
const ITEM_LIST_48: &[i32] = &[1196];
const ITEM_LIST_49: &[i32] = &[313];
const ITEM_LIST_50: &[i32] = &[576];
const ITEM_LIST_51: &[i32] = &[1133];
const ITEM_LIST_52: &[i32] = &[656, 657, 658, 659, 660, 661, 662, 663, 664, 665, 666, 667, 668, 669, 670, 671, 672];
const ITEM_LIST_53: &[i32] = &[620];
const ITEM_LIST_54: &[i32] = &[482];
const ITEM_LIST_55: &[i32] = &[596];
const ITEM_LIST_56: &[i32] = &[1537];
const ITEM_LIST_57: &[i32] = &[407];
const ITEM_LIST_58: &[i32] = &[375];
const ITEM_LIST_59: &[i32] = &[1266];
const ITEM_LIST_60: &[i32] = &[261];
const ITEM_LIST_61: &[i32] = &[1428];
const ITEM_LIST_62: &[i32] = &[1210];
const ITEM_LIST_63: &[i32] = &[700];
const ITEM_LIST_64: &[i32] = &[293];
const ITEM_LIST_65: &[i32] = &[1014];
const ITEM_LIST_66: &[i32] = &[311];
const ITEM_LIST_67: &[i32] = &[572];
const ITEM_LIST_68: &[i32] = &[597];
const ITEM_LIST_69: &[i32] = &[616];
const ITEM_LIST_70: &[i32] = &[1012];
const ITEM_LIST_71: &[i32] = &[1597];
const ITEM_LIST_72: &[i32] = &[120, 124];
const ITEM_LIST_73: &[i32] = &[1001, 1068, 1069, 1070, 1071, 1072, 1073, 1074, 1075, 1076, 1077, 1078, 1079, 1080, 1081, 1082, 1083, 1084, 1085, 1086, 1087, 1088, 1089, 1090, 1091, 1092, 1093, 1094, 1095];
const ITEM_LIST_74: &[i32] = &[824, 1012, 1013, 1014, 1015, 1016, 1018, 1020, 1022, 1023, 1397];
const ITEM_LIST_75: &[i32] = &[1215];
const ITEM_LIST_76: &[i32] = &[1216];
const ITEM_LIST_77: &[i32] = &[654];
const ITEM_LIST_78: &[i32] = &[1145];
const ITEM_LIST_79: &[i32] = &[1372];
const ITEM_LIST_80: &[i32] = &[1371];
const ITEM_LIST_81: &[i32] = &[1146];
const ITEM_LIST_82: &[i32] = &[484];
const ITEM_LIST_83: &[i32] = &[1062];
const ITEM_LIST_84: &[i32] = &[1066];
const ITEM_LIST_85: &[i32] = &[1445];
const ITEM_LIST_86: &[i32] = &[1274];
const ITEM_LIST_87: &[i32] = &[1373];
const ITEM_LIST_88: &[i32] = &[237];
const ITEM_LIST_89: &[i32] = &[1267];
const ITEM_LIST_90: &[i32] = &[1279];
const ITEM_LIST_91: &[i32] = &[1383];
const ITEM_LIST_92: &[i32] = &[1275];
const ITEM_LIST_93: &[i32] = &[973];
const ITEM_LIST_94: &[i32] = &[1193];
const ITEM_LIST_95: &[i32] = &[1403];
const ITEM_LIST_96: &[i32] = &[831];
const ITEM_LIST_97: &[i32] = &[1272];
const ITEM_LIST_98: &[i32] = &[1];
const ITEM_LIST_99: &[i32] = &[1217];
const ITEM_LIST_100: &[i32] = &[1001];
const ITEM_LIST_101: &[i32] = &[824];
const ITEM_LIST_102: &[i32] = &[1189];
const ITEM_LIST_103: &[i32] = &[1273];
const ITEM_LIST_104: &[i32] = &[1269];
const ITEM_LIST_105: &[i32] = &[1271];
const ITEM_LIST_106: &[i32] = &[1064];
const ITEM_LIST_107: &[i32] = &[1442];
const ITEM_LIST_108: &[i32] = &[1441];
const ITEM_LIST_109: &[i32] = &[9, 62, 1537];
const ITEM_LIST_110: &[i32] = &[1143];
const ITEM_LIST_111: &[i32] = &[376];
const ITEM_LIST_112: &[i32] = &[1142];
const ITEM_LIST_113: &[i32] = &[262];
const ITEM_LIST_114: &[i32] = &[1429];
const ITEM_LIST_115: &[i32] = &[1211];
const ITEM_LIST_116: &[i32] = &[701];
const ITEM_LIST_117: &[i32] = &[294];
const ITEM_LIST_118: &[i32] = &[1198];
const ITEM_LIST_119: &[i32] = &[573];
const ITEM_LIST_120: &[i32] = &[617];
const ITEM_LIST_121: &[i32] = &[1020];
const ITEM_LIST_122: &[i32] = &[1134];
const ITEM_LIST_123: &[i32] = &[1148, 1149, 1150];
const ITEM_LIST_124: &[i32] = &[849];
const ITEM_LIST_125: &[i32] = &[1010, 1011];
const ITEM_LIST_126: &[i32] = &[163, 164, 165, 166, 167, 168, 169, 170, 171, 172, 175, 176, 178, 179, 180, 181, 182, 183, 184, 185, 186, 187, 188, 189, 190, 191, 192, 193, 194, 195, 196, 197, 198, 199, 200, 201, 203, 204, 205, 206, 207, 208, 209, 210, 211, 212, 213, 214];
const ITEM_LIST_127: &[i32] = &[1186];
const ITEM_LIST_128: &[i32] = &[1378];
const ITEM_LIST_129: &[i32] = &[163, 164, 165, 166, 167, 168, 169, 170, 171, 172, 178, 179, 180, 181, 182, 183, 184, 185, 186, 187, 190, 191, 192, 193, 194, 195, 196, 197, 198, 199, 203, 204, 205, 206, 207, 208, 209, 210, 211, 212];
const ITEM_LIST_130: &[i32] = &[68];
const ITEM_LIST_131: &[i32] = &[985];
const ITEM_LIST_132: &[i32] = &[183];
const ITEM_LIST_133: &[i32] = &[168, 183, 195, 208];
const ITEM_LIST_134: &[i32] = &[168];
const ITEM_LIST_135: &[i32] = &[966];
const ITEM_LIST_136: &[i32] = &[41];
const ITEM_LIST_137: &[i32] = &[40];
const ITEM_LIST_138: &[i32] = &[155];
const ITEM_LIST_139: &[i32] = &[120];
const ITEM_LIST_140: &[i32] = &[139];
const ITEM_LIST_141: &[i32] = &[819];
const ITEM_LIST_142: &[i32] = &[9];
const ITEM_LIST_143: &[i32] = &[8];
const ITEM_LIST_144: &[i32] = &[364];
const ITEM_LIST_145: &[i32] = &[498];
const ITEM_LIST_146: &[i32] = &[1542];
const ITEM_LIST_147: &[i32] = &[1541];
const ITEM_LIST_148: &[i32] = &[365];
const ITEM_LIST_149: &[i32] = &[557];
const ITEM_LIST_150: &[i32] = &[366];
const ITEM_LIST_151: &[i32] = &[644];
const ITEM_LIST_152: &[i32] = &[490];
const ITEM_LIST_153: &[i32] = &[488];
const ITEM_LIST_154: &[i32] = &[357];
const ITEM_LIST_155: &[i32] = &[234];
const ITEM_LIST_156: &[i32] = &[362];
const ITEM_LIST_157: &[i32] = &[449];
const ITEM_LIST_158: &[i32] = &[28];
const ITEM_LIST_159: &[i32] = &[26];
const ITEM_LIST_160: &[i32] = &[13];
const ITEM_LIST_161: &[i32] = &[22];
const ITEM_LIST_162: &[i32] = &[17];
const ITEM_LIST_163: &[i32] = &[21];
const ITEM_LIST_164: &[i32] = &[12];
const ITEM_LIST_165: &[i32] = &[48];
const ITEM_LIST_166: &[i32] = &[44];
const ITEM_LIST_167: &[i32] = &[1022];
const ITEM_LIST_168: &[i32] = &[112];
const ITEM_LIST_169: &[i32] = &[1010];
const ITEM_LIST_170: &[i32] = &[93];
const ITEM_LIST_171: &[i32] = &[94];
const ITEM_LIST_172: &[i32] = &[55];
const ITEM_LIST_173: &[i32] = &[1582];
const ITEM_LIST_174: &[i32] = &[1015];
const ITEM_LIST_175: &[i32] = &[1484];
const ITEM_LIST_176: &[i32] = &[1490];
const ITEM_LIST_177: &[i32] = &[1260];
const ITEM_LIST_178: &[i32] = &[1262];
const ITEM_LIST_179: &[i32] = &[1190];
const ITEM_LIST_180: &[i32] = &[1415];
const ITEM_LIST_181: &[i32] = &[1097];
const ITEM_LIST_182: &[i32] = &[1400];
const ITEM_LIST_183: &[i32] = &[1191];
const ITEM_LIST_184: &[i32] = &[1457];
const ITEM_LIST_185: &[i32] = &[97];
const ITEM_LIST_186: &[i32] = &[98];
const ITEM_LIST_187: &[i32] = &[1019];
const ITEM_LIST_188: &[i32] = &[124];
const ITEM_LIST_189: &[i32] = &[440];
const ITEM_LIST_190: &[i32] = &[1030, 1031, 1032, 1033, 1034, 1072, 1073, 1074, 1075, 1406, 1449, 1489];
const ITEM_LIST_191: &[i32] = &[455];
const ITEM_LIST_192: &[i32] = &[457];
const ITEM_LIST_193: &[i32] = &[1545];
const ITEM_LIST_194: &[i32] = &[405];
const ITEM_LIST_195: &[i32] = &[836];
const ITEM_LIST_196: &[i32] = &[169];
const ITEM_LIST_197: &[i32] = &[487];
const ITEM_LIST_198: &[i32] = &[1388];
const ITEM_LIST_199: &[i32] = &[74];
const ITEM_LIST_200: &[i32] = &[188];
const ITEM_LIST_201: &[i32] = &[175];
const ITEM_LIST_202: &[i32] = &[175, 188, 200, 213];
const ITEM_LIST_203: &[i32] = &[851];
const ITEM_LIST_204: &[i32] = &[646];
const ITEM_LIST_205: &[i32] = &[236];
const ITEM_LIST_206: &[i32] = &[259];
const ITEM_LIST_207: &[i32] = &[1426];
const ITEM_LIST_208: &[i32] = &[1208];
const ITEM_LIST_209: &[i32] = &[698];
const ITEM_LIST_210: &[i32] = &[291];
const ITEM_LIST_211: &[i32] = &[1212];
const ITEM_LIST_212: &[i32] = &[315];
const ITEM_LIST_213: &[i32] = &[570];
const ITEM_LIST_214: &[i32] = &[614];
const ITEM_LIST_215: &[i32] = &[69];
const ITEM_LIST_216: &[i32] = &[987];
const ITEM_LIST_217: &[i32] = &[184];
const ITEM_LIST_218: &[i32] = &[170, 184, 196, 210];
const ITEM_LIST_219: &[i32] = &[170];
const ITEM_LIST_220: &[i32] = &[1398];
const ITEM_LIST_221: &[i32] = &[639];
const ITEM_LIST_222: &[i32] = &[1142, 1598, 1599, 1600, 1601, 1602, 1603, 1604, 1605, 1606, 1607, 1608, 1609, 1610, 1611, 1612, 1613, 1614, 1615, 1616, 1617, 1618, 1619, 1620];
const ITEM_LIST_223: &[i32] = &[10];
const ITEM_LIST_224: &[i32] = &[871];
const ITEM_LIST_225: &[i32] = &[129];
const ITEM_LIST_226: &[i32] = &[108];
const ITEM_LIST_227: &[i32] = &[107];
const ITEM_LIST_228: &[i32] = &[1008];
const ITEM_LIST_229: &[i32] = &[434];
const ITEM_LIST_230: &[i32] = &[1144];
const ITEM_LIST_231: &[i32] = &[1257];
const ITEM_LIST_232: &[i32] = &[328];
const ITEM_LIST_233: &[i32] = &[1571];
const ITEM_LIST_234: &[i32] = &[1581];
const ITEM_LIST_235: &[i32] = &[1219, 1220, 1221, 1222, 1223, 1224, 1225, 1226, 1227, 1228, 1229, 1230, 1231, 1232, 1233];
const ITEM_LIST_236: &[i32] = &[580, 581, 582, 583, 584, 585, 586, 587, 588, 589, 590, 591, 592, 593, 594];
const ITEM_LIST_237: &[i32] = &[1170, 1171, 1172, 1173, 1174, 1175, 1176, 1177, 1178, 1179, 1180, 1181, 1182, 1183, 1184];
const ITEM_LIST_238: &[i32] = &[950, 951, 952, 953, 954, 955, 956, 957, 958, 959, 960, 961, 962, 963, 964];
const ITEM_LIST_239: &[i32] = &[250, 251, 252, 253, 254, 255, 256, 257, 258, 259, 260, 261, 262, 263, 264];
const ITEM_LIST_240: &[i32] = &[282, 283, 284, 285, 286, 287, 288, 289, 290, 291, 292, 293, 294, 295, 296];
const ITEM_LIST_241: &[i32] = &[266, 267, 268, 269, 270, 271, 272, 273, 274, 275, 276, 277, 278, 279, 280];
const ITEM_LIST_242: &[i32] = &[1219, 1220, 1221, 1222, 1223, 1224, 1225, 1226, 1227, 1228, 1229, 1231, 1232, 1233, 1234];
const ITEM_LIST_243: &[i32] = &[580, 581, 582, 583, 584, 585, 586, 587, 588, 589, 590, 592, 593, 594, 595];
const ITEM_LIST_244: &[i32] = &[1170, 1171, 1172, 1173, 1174, 1175, 1176, 1177, 1178, 1179, 1180, 1182, 1183, 1184, 1185];
const ITEM_LIST_245: &[i32] = &[950, 951, 952, 953, 954, 955, 956, 957, 958, 959, 960, 962, 963, 964, 965];
const ITEM_LIST_246: &[i32] = &[250, 251, 252, 253, 254, 255, 256, 257, 258, 259, 260, 262, 263, 264, 265];
const ITEM_LIST_247: &[i32] = &[282, 283, 284, 285, 286, 287, 288, 289, 290, 291, 292, 294, 295, 296, 297];
const ITEM_LIST_248: &[i32] = &[266, 267, 268, 269, 270, 271, 272, 273, 274, 275, 276, 278, 279, 280, 281];
const ITEM_LIST_249: &[i32] = &[1219, 1220, 1221, 1222, 1223, 1224, 1225, 1226, 1227, 1228, 1229, 1230, 1232, 1233, 1234];
const ITEM_LIST_250: &[i32] = &[580, 581, 582, 583, 584, 585, 586, 587, 588, 589, 590, 591, 593, 594, 595];
const ITEM_LIST_251: &[i32] = &[1170, 1171, 1172, 1173, 1174, 1175, 1176, 1177, 1178, 1179, 1180, 1181, 1183, 1184, 1185];
const ITEM_LIST_252: &[i32] = &[950, 951, 952, 953, 954, 955, 956, 957, 958, 959, 960, 961, 963, 964, 965];
const ITEM_LIST_253: &[i32] = &[250, 251, 252, 253, 254, 255, 256, 257, 258, 259, 260, 261, 263, 264, 265];
const ITEM_LIST_254: &[i32] = &[282, 283, 284, 285, 286, 287, 288, 289, 290, 291, 292, 293, 295, 296, 297];
const ITEM_LIST_255: &[i32] = &[266, 267, 268, 269, 270, 271, 272, 273, 274, 275, 276, 277, 279, 280, 281];
const ITEM_LIST_256: &[i32] = &[1219, 1220, 1221, 1222, 1223, 1224, 1225, 1226, 1227, 1229, 1230, 1231, 1232, 1233, 1234];
const ITEM_LIST_257: &[i32] = &[580, 581, 582, 583, 584, 585, 586, 587, 588, 590, 591, 592, 593, 594, 595];
const ITEM_LIST_258: &[i32] = &[1170, 1171, 1172, 1173, 1174, 1175, 1176, 1177, 1178, 1180, 1181, 1182, 1183, 1184, 1185];
const ITEM_LIST_259: &[i32] = &[950, 951, 952, 953, 954, 955, 956, 957, 958, 960, 961, 962, 963, 964, 965];
const ITEM_LIST_260: &[i32] = &[250, 251, 252, 253, 254, 255, 256, 257, 258, 260, 261, 262, 263, 264, 265];
const ITEM_LIST_261: &[i32] = &[282, 283, 284, 285, 286, 287, 288, 289, 290, 292, 293, 294, 295, 296, 297];
const ITEM_LIST_262: &[i32] = &[266, 267, 268, 269, 270, 271, 272, 273, 274, 276, 277, 278, 279, 280, 281];
const ITEM_LIST_263: &[i32] = &[1206];
const ITEM_LIST_264: &[i32] = &[1219, 1220, 1221, 1222, 1223, 1224, 1225, 1227, 1228, 1229, 1230, 1231, 1232, 1233, 1234];
const ITEM_LIST_265: &[i32] = &[580, 581, 582, 583, 584, 585, 586, 588, 589, 590, 591, 592, 593, 594, 595];
const ITEM_LIST_266: &[i32] = &[1170, 1171, 1172, 1173, 1174, 1175, 1176, 1178, 1179, 1180, 1181, 1182, 1183, 1184, 1185];
const ITEM_LIST_267: &[i32] = &[950, 951, 952, 953, 954, 955, 956, 958, 959, 960, 961, 962, 963, 964, 965];
const ITEM_LIST_268: &[i32] = &[250, 251, 252, 253, 254, 255, 256, 258, 259, 260, 261, 262, 263, 264, 265];
const ITEM_LIST_269: &[i32] = &[282, 283, 284, 285, 286, 287, 288, 290, 291, 292, 293, 294, 295, 296, 297];
const ITEM_LIST_270: &[i32] = &[266, 267, 268, 269, 270, 271, 272, 274, 275, 276, 277, 278, 279, 280, 281];
const ITEM_LIST_271: &[i32] = &[1219, 1220, 1221, 1222, 1223, 1224, 1225, 1226, 1227, 1228, 1229, 1230, 1231, 1233, 1234];
const ITEM_LIST_272: &[i32] = &[580, 581, 582, 583, 584, 585, 586, 587, 588, 589, 590, 591, 592, 594, 595];
const ITEM_LIST_273: &[i32] = &[1170, 1171, 1172, 1173, 1174, 1175, 1176, 1177, 1178, 1179, 1180, 1181, 1182, 1184, 1185];
const ITEM_LIST_274: &[i32] = &[950, 951, 952, 953, 954, 955, 956, 957, 958, 959, 960, 961, 962, 964, 965];
const ITEM_LIST_275: &[i32] = &[250, 251, 252, 253, 254, 255, 256, 257, 258, 259, 260, 261, 262, 264, 265];
const ITEM_LIST_276: &[i32] = &[282, 283, 284, 285, 286, 287, 288, 289, 290, 291, 292, 293, 294, 296, 297];
const ITEM_LIST_277: &[i32] = &[266, 267, 268, 269, 270, 271, 272, 273, 274, 275, 276, 277, 278, 280, 281];
const ITEM_LIST_278: &[i32] = &[1202];
const ITEM_LIST_279: &[i32] = &[1219, 1220, 1221, 1223, 1224, 1225, 1226, 1227, 1228, 1229, 1230, 1231, 1232, 1233, 1234];
const ITEM_LIST_280: &[i32] = &[580, 581, 582, 584, 585, 586, 587, 588, 589, 590, 591, 592, 593, 594, 595];
const ITEM_LIST_281: &[i32] = &[1170, 1171, 1172, 1174, 1175, 1176, 1177, 1178, 1179, 1180, 1181, 1182, 1183, 1184, 1185];
const ITEM_LIST_282: &[i32] = &[950, 951, 952, 954, 955, 956, 957, 958, 959, 960, 961, 962, 963, 964, 965];
const ITEM_LIST_283: &[i32] = &[250, 251, 252, 254, 255, 256, 257, 258, 259, 260, 261, 262, 263, 264, 265];
const ITEM_LIST_284: &[i32] = &[282, 283, 284, 286, 287, 288, 289, 290, 291, 292, 293, 294, 295, 296, 297];
const ITEM_LIST_285: &[i32] = &[266, 267, 268, 270, 271, 272, 273, 274, 275, 276, 277, 278, 279, 280, 281];
const ITEM_LIST_286: &[i32] = &[1207];
const ITEM_LIST_287: &[i32] = &[1219, 1220, 1221, 1222, 1223, 1224, 1225, 1226, 1228, 1229, 1230, 1231, 1232, 1233, 1234];
const ITEM_LIST_288: &[i32] = &[580, 581, 582, 583, 584, 585, 586, 587, 589, 590, 591, 592, 593, 594, 595];
const ITEM_LIST_289: &[i32] = &[1170, 1171, 1172, 1173, 1174, 1175, 1176, 1177, 1179, 1180, 1181, 1182, 1183, 1184, 1185];
const ITEM_LIST_290: &[i32] = &[950, 951, 952, 953, 954, 955, 956, 957, 959, 960, 961, 962, 963, 964, 965];
const ITEM_LIST_291: &[i32] = &[250, 251, 252, 253, 254, 255, 256, 257, 259, 260, 261, 262, 263, 264, 265];
const ITEM_LIST_292: &[i32] = &[282, 283, 284, 285, 286, 287, 288, 289, 291, 292, 293, 294, 295, 296, 297];
const ITEM_LIST_293: &[i32] = &[266, 267, 268, 269, 270, 271, 272, 273, 275, 276, 277, 278, 279, 280, 281];
const ITEM_LIST_294: &[i32] = &[1204];
const ITEM_LIST_295: &[i32] = &[1219, 1220, 1221, 1222, 1223, 1225, 1226, 1227, 1228, 1229, 1230, 1231, 1232, 1233, 1234];
const ITEM_LIST_296: &[i32] = &[580, 581, 582, 583, 584, 586, 587, 588, 589, 590, 591, 592, 593, 594, 595];
const ITEM_LIST_297: &[i32] = &[1170, 1171, 1172, 1173, 1174, 1176, 1177, 1178, 1179, 1180, 1181, 1182, 1183, 1184, 1185];
const ITEM_LIST_298: &[i32] = &[950, 951, 952, 953, 954, 956, 957, 958, 959, 960, 961, 962, 963, 964, 965];
const ITEM_LIST_299: &[i32] = &[250, 251, 252, 253, 254, 256, 257, 258, 259, 260, 261, 262, 263, 264, 265];
const ITEM_LIST_300: &[i32] = &[282, 283, 284, 285, 286, 288, 289, 290, 291, 292, 293, 294, 295, 296, 297];
const ITEM_LIST_301: &[i32] = &[266, 267, 268, 269, 270, 272, 273, 274, 275, 276, 277, 278, 279, 280, 281];
const ITEM_LIST_302: &[i32] = &[1201];
const ITEM_LIST_303: &[i32] = &[1219, 1220, 1222, 1223, 1224, 1225, 1226, 1227, 1228, 1229, 1230, 1231, 1232, 1233, 1234];
const ITEM_LIST_304: &[i32] = &[580, 581, 583, 584, 585, 586, 587, 588, 589, 590, 591, 592, 593, 594, 595];
const ITEM_LIST_305: &[i32] = &[1170, 1171, 1173, 1174, 1175, 1176, 1177, 1178, 1179, 1180, 1181, 1182, 1183, 1184, 1185];
const ITEM_LIST_306: &[i32] = &[950, 951, 953, 954, 955, 956, 957, 958, 959, 960, 961, 962, 963, 964, 965];
const ITEM_LIST_307: &[i32] = &[250, 251, 253, 254, 255, 256, 257, 258, 259, 260, 261, 262, 263, 264, 265];
const ITEM_LIST_308: &[i32] = &[282, 283, 285, 286, 287, 288, 289, 290, 291, 292, 293, 294, 295, 296, 297];
const ITEM_LIST_309: &[i32] = &[266, 267, 269, 270, 271, 272, 273, 274, 275, 276, 277, 278, 279, 280, 281];
const ITEM_LIST_310: &[i32] = &[1200];
const ITEM_LIST_311: &[i32] = &[1219, 1221, 1222, 1223, 1224, 1225, 1226, 1227, 1228, 1229, 1230, 1231, 1232, 1233, 1234];
const ITEM_LIST_312: &[i32] = &[580, 582, 583, 584, 585, 586, 587, 588, 589, 590, 591, 592, 593, 594, 595];
const ITEM_LIST_313: &[i32] = &[1170, 1172, 1173, 1174, 1175, 1176, 1177, 1178, 1179, 1180, 1181, 1182, 1183, 1184, 1185];
const ITEM_LIST_314: &[i32] = &[950, 952, 953, 954, 955, 956, 957, 958, 959, 960, 961, 962, 963, 964, 965];
const ITEM_LIST_315: &[i32] = &[250, 252, 253, 254, 255, 256, 257, 258, 259, 260, 261, 262, 263, 264, 265];
const ITEM_LIST_316: &[i32] = &[282, 284, 285, 286, 287, 288, 289, 290, 291, 292, 293, 294, 295, 296, 297];
const ITEM_LIST_317: &[i32] = &[266, 268, 269, 270, 271, 272, 273, 274, 275, 276, 277, 278, 279, 280, 281];
const ITEM_LIST_318: &[i32] = &[1205];
const ITEM_LIST_319: &[i32] = &[1219, 1220, 1221, 1222, 1223, 1224, 1226, 1227, 1228, 1229, 1230, 1231, 1232, 1233, 1234];
const ITEM_LIST_320: &[i32] = &[580, 581, 582, 583, 584, 585, 587, 588, 589, 590, 591, 592, 593, 594, 595];
const ITEM_LIST_321: &[i32] = &[1170, 1171, 1172, 1173, 1174, 1175, 1177, 1178, 1179, 1180, 1181, 1182, 1183, 1184, 1185];
const ITEM_LIST_322: &[i32] = &[950, 951, 952, 953, 954, 955, 957, 958, 959, 960, 961, 962, 963, 964, 965];
const ITEM_LIST_323: &[i32] = &[250, 251, 252, 253, 254, 255, 257, 258, 259, 260, 261, 262, 263, 264, 265];
const ITEM_LIST_324: &[i32] = &[282, 283, 284, 285, 286, 287, 289, 290, 291, 292, 293, 294, 295, 296, 297];
const ITEM_LIST_325: &[i32] = &[266, 267, 268, 269, 270, 271, 273, 274, 275, 276, 277, 278, 279, 280, 281];
const ITEM_LIST_326: &[i32] = &[1209];
const ITEM_LIST_327: &[i32] = &[1219, 1220, 1221, 1222, 1223, 1224, 1225, 1226, 1227, 1228, 1230, 1231, 1232, 1233, 1234];
const ITEM_LIST_328: &[i32] = &[580, 581, 582, 583, 584, 585, 586, 587, 588, 589, 591, 592, 593, 594, 595];
const ITEM_LIST_329: &[i32] = &[1170, 1171, 1172, 1173, 1174, 1175, 1176, 1177, 1178, 1179, 1181, 1182, 1183, 1184, 1185];
const ITEM_LIST_330: &[i32] = &[950, 951, 952, 953, 954, 955, 956, 957, 958, 959, 961, 962, 963, 964, 965];
const ITEM_LIST_331: &[i32] = &[250, 251, 252, 253, 254, 255, 256, 257, 258, 259, 261, 262, 263, 264, 265];
const ITEM_LIST_332: &[i32] = &[282, 283, 284, 285, 286, 287, 288, 289, 290, 291, 293, 294, 295, 296, 297];
const ITEM_LIST_333: &[i32] = &[266, 267, 268, 269, 270, 271, 272, 273, 274, 275, 277, 278, 279, 280, 281];
const ITEM_LIST_334: &[i32] = &[1213];
const ITEM_LIST_335: &[i32] = &[1219, 1220, 1221, 1222, 1223, 1224, 1225, 1226, 1227, 1228, 1229, 1230, 1231, 1232, 1234];
const ITEM_LIST_336: &[i32] = &[580, 581, 582, 583, 584, 585, 586, 587, 588, 589, 590, 591, 592, 593, 595];
const ITEM_LIST_337: &[i32] = &[1170, 1171, 1172, 1173, 1174, 1175, 1176, 1177, 1178, 1179, 1180, 1181, 1182, 1183, 1185];
const ITEM_LIST_338: &[i32] = &[950, 951, 952, 953, 954, 955, 956, 957, 958, 959, 960, 961, 962, 963, 965];
const ITEM_LIST_339: &[i32] = &[250, 251, 252, 253, 254, 255, 256, 257, 258, 259, 260, 261, 262, 263, 265];
const ITEM_LIST_340: &[i32] = &[282, 283, 284, 285, 286, 287, 288, 289, 290, 291, 292, 293, 294, 295, 297];
const ITEM_LIST_341: &[i32] = &[266, 267, 268, 269, 270, 271, 272, 273, 274, 275, 276, 277, 278, 279, 281];
const ITEM_LIST_342: &[i32] = &[1199];
const ITEM_LIST_343: &[i32] = &[1220, 1221, 1222, 1223, 1224, 1225, 1226, 1227, 1228, 1229, 1230, 1231, 1232, 1233, 1234];
const ITEM_LIST_344: &[i32] = &[581, 582, 583, 584, 585, 586, 587, 588, 589, 590, 591, 592, 593, 594, 595];
const ITEM_LIST_345: &[i32] = &[1171, 1172, 1173, 1174, 1175, 1176, 1177, 1178, 1179, 1180, 1181, 1182, 1183, 1184, 1185];
const ITEM_LIST_346: &[i32] = &[951, 952, 953, 954, 955, 956, 957, 958, 959, 960, 961, 962, 963, 964, 965];
const ITEM_LIST_347: &[i32] = &[251, 252, 253, 254, 255, 256, 257, 258, 259, 260, 261, 262, 263, 264, 265];
const ITEM_LIST_348: &[i32] = &[283, 284, 285, 286, 287, 288, 289, 290, 291, 292, 293, 294, 295, 296, 297];
const ITEM_LIST_349: &[i32] = &[267, 268, 269, 270, 271, 272, 273, 274, 275, 276, 277, 278, 279, 280, 281];
const ITEM_LIST_350: &[i32] = &[1203];
const ITEM_LIST_351: &[i32] = &[1219, 1220, 1221, 1222, 1224, 1225, 1226, 1227, 1228, 1229, 1230, 1231, 1232, 1233, 1234];
const ITEM_LIST_352: &[i32] = &[580, 581, 582, 583, 585, 586, 587, 588, 589, 590, 591, 592, 593, 594, 595];
const ITEM_LIST_353: &[i32] = &[1170, 1171, 1172, 1173, 1175, 1176, 1177, 1178, 1179, 1180, 1181, 1182, 1183, 1184, 1185];
const ITEM_LIST_354: &[i32] = &[950, 951, 952, 953, 955, 956, 957, 958, 959, 960, 961, 962, 963, 964, 965];
const ITEM_LIST_355: &[i32] = &[250, 251, 252, 253, 255, 256, 257, 258, 259, 260, 261, 262, 263, 264, 265];
const ITEM_LIST_356: &[i32] = &[282, 283, 284, 285, 287, 288, 289, 290, 291, 292, 293, 294, 295, 296, 297];
const ITEM_LIST_357: &[i32] = &[266, 267, 268, 269, 271, 272, 273, 274, 275, 276, 277, 278, 279, 280, 281];
const ITEM_LIST_358: &[i32] = &[514];
const ITEM_LIST_359: &[i32] = &[1013];
const ITEM_LIST_360: &[i32] = &[104];
const ITEM_LIST_361: &[i32] = &[103];
const ITEM_LIST_362: &[i32] = &[1278];
const ITEM_LIST_363: &[i32] = &[1435];
const ITEM_LIST_364: &[i32] = &[510];
const ITEM_LIST_365: &[i32] = &[509];
const ITEM_LIST_366: &[i32] = &[1265];
const ITEM_LIST_367: &[i32] = &[156];
const ITEM_LIST_368: &[i32] = &[121];
const ITEM_LIST_369: &[i32] = &[140];
const ITEM_LIST_370: &[i32] = &[1585];
const ITEM_LIST_371: &[i32] = &[317];
const ITEM_LIST_372: &[i32] = &[1394];
const ITEM_LIST_373: &[i32] = &[1384, 1385, 1386, 1387, 1388, 1389, 1390];
const ITEM_LIST_374: &[i32] = &[1369];
const ITEM_LIST_375: &[i32] = &[1268];
const ITEM_LIST_376: &[i32] = &[1199, 1200, 1201, 1202, 1203, 1204, 1205, 1206, 1207, 1208, 1209, 1210, 1211, 1212, 1213, 1214];
const ITEM_LIST_377: &[i32] = &[1596];
const ITEM_LIST_378: &[i32] = &[310];
const ITEM_LIST_379: &[i32] = &[88, 91];
const ITEM_LIST_380: &[i32] = &[1256];
const ITEM_LIST_381: &[i32] = &[1375];
const ITEM_LIST_382: &[i32] = &[1197];
const ITEM_LIST_383: &[i32] = &[100];
const ITEM_LIST_384: &[i32] = &[99];
const ITEM_LIST_385: &[i32] = &[109];
const ITEM_LIST_386: &[i32] = &[1021];
const ITEM_LIST_387: &[i32] = &[128];
const ITEM_LIST_388: &[i32] = &[1040, 1041, 1042, 1043, 1044, 1088, 1089, 1090, 1091, 1408, 1451, 1486];
const ITEM_LIST_389: &[i32] = &[1007];
const ITEM_LIST_390: &[i32] = &[298];
const ITEM_LIST_391: &[i32] = &[2];
const ITEM_LIST_392: &[i32] = &[257];
const ITEM_LIST_393: &[i32] = &[1424];
const ITEM_LIST_394: &[i32] = &[696];
const ITEM_LIST_395: &[i32] = &[289];
const ITEM_LIST_396: &[i32] = &[301];
const ITEM_LIST_397: &[i32] = &[568];
const ITEM_LIST_398: &[i32] = &[612];
const ITEM_LIST_399: &[i32] = &[263];
const ITEM_LIST_400: &[i32] = &[1430];
const ITEM_LIST_401: &[i32] = &[702];
const ITEM_LIST_402: &[i32] = &[295];
const ITEM_LIST_403: &[i32] = &[413];
const ITEM_LIST_404: &[i32] = &[574];
const ITEM_LIST_405: &[i32] = &[618];
const ITEM_LIST_406: &[i32] = &[355];
const ITEM_LIST_407: &[i32] = &[1533];
const ITEM_LIST_408: &[i32] = &[832];
const ITEM_LIST_409: &[i32] = &[1270];
const ITEM_LIST_410: &[i32] = &[834];
const ITEM_LIST_411: &[i32] = &[1595];
const ITEM_LIST_412: &[i32] = &[1456];
const ITEM_LIST_413: &[i32] = &[96];
const ITEM_LIST_414: &[i32] = &[95];
const ITEM_LIST_415: &[i32] = &[1017];
const ITEM_LIST_416: &[i32] = &[1045, 1046, 1047, 1048, 1049, 1076, 1077, 1078, 1079, 1080, 1081, 1082, 1083, 1407, 1450, 1485];
const ITEM_LIST_417: &[i32] = &[431];
const ITEM_LIST_418: &[i32] = &[395];
const ITEM_LIST_419: &[i32] = &[66];
const ITEM_LIST_420: &[i32] = &[981];
const ITEM_LIST_421: &[i32] = &[181];
const ITEM_LIST_422: &[i32] = &[166, 181, 193, 206];
const ITEM_LIST_423: &[i32] = &[166];
const ITEM_LIST_424: &[i32] = &[233];
const ITEM_LIST_425: &[i32] = &[106];
const ITEM_LIST_426: &[i32] = &[105];
const ITEM_LIST_427: &[i32] = &[215, 216, 217, 218, 219, 220, 221, 222, 223, 224, 225, 226, 227, 228];
const ITEM_LIST_428: &[i32] = &[1404];
const ITEM_LIST_429: &[i32] = &[1071];
const ITEM_LIST_430: &[i32] = &[1069];
const ITEM_LIST_431: &[i32] = &[1068];
const ITEM_LIST_432: &[i32] = &[1411];
const ITEM_LIST_433: &[i32] = &[1070];
const ITEM_LIST_434: &[i32] = &[390];
const ITEM_LIST_435: &[i32] = &[253];
const ITEM_LIST_436: &[i32] = &[1420];
const ITEM_LIST_437: &[i32] = &[692];
const ITEM_LIST_438: &[i32] = &[285];
const ITEM_LIST_439: &[i32] = &[303];
const ITEM_LIST_440: &[i32] = &[564];
const ITEM_LIST_441: &[i32] = &[608];
const ITEM_LIST_442: &[i32] = &[258];
const ITEM_LIST_443: &[i32] = &[1425];
const ITEM_LIST_444: &[i32] = &[697];
const ITEM_LIST_445: &[i32] = &[290];
const ITEM_LIST_446: &[i32] = &[305];
const ITEM_LIST_447: &[i32] = &[308];
const ITEM_LIST_448: &[i32] = &[569];
const ITEM_LIST_449: &[i32] = &[613];
const ITEM_LIST_450: &[i32] = &[255];
const ITEM_LIST_451: &[i32] = &[1422];
const ITEM_LIST_452: &[i32] = &[694];
const ITEM_LIST_453: &[i32] = &[287];
const ITEM_LIST_454: &[i32] = &[249];
const ITEM_LIST_455: &[i32] = &[566];
const ITEM_LIST_456: &[i32] = &[610];
const ITEM_LIST_457: &[i32] = &[452];
const ITEM_LIST_458: &[i32] = &[116];
const ITEM_LIST_459: &[i32] = &[252];
const ITEM_LIST_460: &[i32] = &[1419];
const ITEM_LIST_461: &[i32] = &[691];
const ITEM_LIST_462: &[i32] = &[284];
const ITEM_LIST_463: &[i32] = &[304];
const ITEM_LIST_464: &[i32] = &[600];
const ITEM_LIST_465: &[i32] = &[563];
const ITEM_LIST_466: &[i32] = &[607];
const ITEM_LIST_467: &[i32] = &[1147];
const ITEM_LIST_468: &[i32] = &[71];
const ITEM_LIST_469: &[i32] = &[991];
const ITEM_LIST_470: &[i32] = &[186];
const ITEM_LIST_471: &[i32] = &[171, 186, 198, 211];
const ITEM_LIST_472: &[i32] = &[171];
const ITEM_LIST_473: &[i32] = &[1151];
const ITEM_LIST_474: &[i32] = &[1238, 1239, 1240, 1241, 1242, 1243, 1244, 1245, 1246, 1247, 1248, 1249, 1250, 1251, 1252, 1253, 1254];
const ITEM_LIST_475: &[i32] = &[1382];
const ITEM_LIST_476: &[i32] = &[1238];
const ITEM_LIST_477: &[i32] = &[1101];
const ITEM_LIST_478: &[i32] = &[333];
const ITEM_LIST_479: &[i32] = &[393];
const ITEM_LIST_480: &[i32] = &[450];
const ITEM_LIST_481: &[i32] = &[454];
const ITEM_LIST_482: &[i32] = &[453];
const ITEM_LIST_483: &[i32] = &[59];
const ITEM_LIST_484: &[i32] = &[173];
const ITEM_LIST_485: &[i32] = &[317, 318, 319];
const ITEM_LIST_486: &[i32] = &[1482];
const ITEM_LIST_487: &[i32] = &[1268, 1456, 1457];
const ITEM_LIST_488: &[i32] = &[433];
const ITEM_LIST_489: &[i32] = &[1396];
const ITEM_LIST_490: &[i32] = &[1579];
const ITEM_LIST_491: &[i32] = &[1053];
const ITEM_LIST_492: &[i32] = &[1023];
const ITEM_LIST_493: &[i32] = &[1087];
const ITEM_LIST_494: &[i32] = &[1085];
const ITEM_LIST_495: &[i32] = &[1084];
const ITEM_LIST_496: &[i32] = &[1054];
const ITEM_LIST_497: &[i32] = &[1409];
const ITEM_LIST_498: &[i32] = &[1024];
const ITEM_LIST_499: &[i32] = &[130];
const ITEM_LIST_500: &[i32] = &[1086];
const ITEM_LIST_501: &[i32] = &[1487];
const ITEM_LIST_502: &[i32] = &[1052];
const ITEM_LIST_503: &[i32] = &[111];
const ITEM_LIST_504: &[i32] = &[1051];
const ITEM_LIST_505: &[i32] = &[1452];
const ITEM_LIST_506: &[i32] = &[1050];
const ITEM_LIST_507: &[i32] = &[63];
const ITEM_LIST_508: &[i32] = &[975];
const ITEM_LIST_509: &[i32] = &[178];
const ITEM_LIST_510: &[i32] = &[163, 178, 190, 203];
const ITEM_LIST_511: &[i32] = &[163];
const ITEM_LIST_512: &[i32] = &[251];
const ITEM_LIST_513: &[i32] = &[1418];
const ITEM_LIST_514: &[i32] = &[690];
const ITEM_LIST_515: &[i32] = &[283];
const ITEM_LIST_516: &[i32] = &[300];
const ITEM_LIST_517: &[i32] = &[307];
const ITEM_LIST_518: &[i32] = &[314];
const ITEM_LIST_519: &[i32] = &[562];
const ITEM_LIST_520: &[i32] = &[606];
const ITEM_LIST_521: &[i32] = &[158];
const ITEM_LIST_522: &[i32] = &[123];
const ITEM_LIST_523: &[i32] = &[142];
const ITEM_LIST_524: &[i32] = &[411];
const ITEM_LIST_525: &[i32] = &[250, 251, 252, 253, 254, 255, 256, 257, 258, 259, 260, 261, 262, 263, 264, 265];
const ITEM_LIST_526: &[i32] = &[336];
const ITEM_LIST_527: &[i32] = &[70];
const ITEM_LIST_528: &[i32] = &[989];
const ITEM_LIST_529: &[i32] = &[185];
const ITEM_LIST_530: &[i32] = &[169, 185, 197, 209];
const ITEM_LIST_531: &[i32] = &[327];
const ITEM_LIST_532: &[i32] = &[256];
const ITEM_LIST_533: &[i32] = &[1423];
const ITEM_LIST_534: &[i32] = &[695];
const ITEM_LIST_535: &[i32] = &[288];
const ITEM_LIST_536: &[i32] = &[414];
const ITEM_LIST_537: &[i32] = &[602];
const ITEM_LIST_538: &[i32] = &[329];
const ITEM_LIST_539: &[i32] = &[309];
const ITEM_LIST_540: &[i32] = &[567];
const ITEM_LIST_541: &[i32] = &[611];
const ITEM_LIST_542: &[i32] = &[7];
const ITEM_LIST_543: &[i32] = &[436];
const ITEM_LIST_544: &[i32] = &[5];
const ITEM_LIST_545: &[i32] = &[3];
const ITEM_LIST_546: &[i32] = &[31];
const ITEM_LIST_547: &[i32] = &[72];
const ITEM_LIST_548: &[i32] = &[993];
const ITEM_LIST_549: &[i32] = &[187];
const ITEM_LIST_550: &[i32] = &[172, 187, 199, 212];
const ITEM_LIST_551: &[i32] = &[172];
const ITEM_LIST_552: &[i32] = &[1434];
const ITEM_LIST_553: &[i32] = &[638];
const ITEM_LIST_554: &[i32] = &[637];
const ITEM_LIST_555: &[i32] = &[430];
const ITEM_LIST_556: &[i32] = &[260];
const ITEM_LIST_557: &[i32] = &[1427];
const ITEM_LIST_558: &[i32] = &[699];
const ITEM_LIST_559: &[i32] = &[292];
const ITEM_LIST_560: &[i32] = &[571];
const ITEM_LIST_561: &[i32] = &[615];
const ITEM_LIST_562: &[i32] = &[368];
const ITEM_LIST_563: &[i32] = &[399];
const ITEM_LIST_564: &[i32] = &[399, 400];
const ITEM_LIST_565: &[i32] = &[110];
const ITEM_LIST_566: &[i32] = &[556, 557, 559];
const ITEM_LIST_567: &[i32] = &[1380];
const ITEM_LIST_568: &[i32] = &[1401];
const ITEM_LIST_569: &[i32] = &[318];
const ITEM_LIST_570: &[i32] = &[319];
const ITEM_LIST_571: &[i32] = &[1594];
const ITEM_LIST_572: &[i32] = &[114];
const ITEM_LIST_573: &[i32] = &[115];
const ITEM_LIST_574: &[i32] = &[113];
const ITEM_LIST_575: &[i32] = &[1577];
const ITEM_LIST_576: &[i32] = &[264];
const ITEM_LIST_577: &[i32] = &[1431];
const ITEM_LIST_578: &[i32] = &[703];
const ITEM_LIST_579: &[i32] = &[296];
const ITEM_LIST_580: &[i32] = &[302];
const ITEM_LIST_581: &[i32] = &[601];
const ITEM_LIST_582: &[i32] = &[306];
const ITEM_LIST_583: &[i32] = &[575];
const ITEM_LIST_584: &[i32] = &[653];
const ITEM_LIST_585: &[i32] = &[91];
const ITEM_LIST_586: &[i32] = &[644, 645];
const ITEM_LIST_587: &[i32] = &[644, 645, 646];
const ITEM_LIST_588: &[i32] = &[619];
const ITEM_LIST_589: &[i32] = &[826];
const ITEM_LIST_590: &[i32] = &[102];
const ITEM_LIST_591: &[i32] = &[101];
const ITEM_LIST_592: &[i32] = &[441];
const ITEM_LIST_593: &[i32] = &[486];
const ITEM_LIST_594: &[i32] = &[1397];
const ITEM_LIST_595: &[i32] = &[1536];
const ITEM_LIST_596: &[i32] = &[1589];
const ITEM_LIST_597: &[i32] = &[234, 235];
const ITEM_LIST_598: &[i32] = &[234, 235, 236];
const ITEM_LIST_599: &[i32] = &[1399];
const ITEM_LIST_600: &[i32] = &[1580];
const ITEM_LIST_601: &[i32] = &[1592];
const ITEM_LIST_602: &[i32] = &[1417, 1418, 1419, 1420, 1421, 1422, 1423, 1424, 1425, 1426, 1427, 1428, 1429, 1430, 1431, 1432];
const ITEM_LIST_603: &[i32] = &[1446];
const ITEM_LIST_604: &[i32] = &[1455];
const ITEM_LIST_605: &[i32] = &[1593];
const ITEM_LIST_606: &[i32] = &[1385];
const ITEM_LIST_607: &[i32] = &[372];
const ITEM_LIST_608: &[i32] = &[373];
const ITEM_LIST_609: &[i32] = &[374];
const ITEM_LIST_610: &[i32] = &[1588];
const ITEM_LIST_611: &[i32] = &[412];
const ITEM_LIST_612: &[i32] = &[1132];
const ITEM_LIST_613: &[i32] = &[434, 435];
const ITEM_LIST_614: &[i32] = &[439];
const ITEM_LIST_615: &[i32] = &[1009];
const ITEM_LIST_616: &[i32] = &[1590];
const ITEM_LIST_617: &[i32] = &[230];
const ITEM_LIST_618: &[i32] = &[64];
const ITEM_LIST_619: &[i32] = &[977];
const ITEM_LIST_620: &[i32] = &[179];
const ITEM_LIST_621: &[i32] = &[164, 179, 191, 204];
const ITEM_LIST_622: &[i32] = &[164];
const ITEM_LIST_623: &[i32] = &[829];
const ITEM_LIST_624: &[i32] = &[579];
const ITEM_LIST_625: &[i32] = &[189];
const ITEM_LIST_626: &[i32] = &[35];
const ITEM_LIST_627: &[i32] = &[1572];
const ITEM_LIST_628: &[i32] = &[299];
const ITEM_LIST_629: &[i32] = &[312];
const ITEM_LIST_630: &[i32] = &[415];
const ITEM_LIST_631: &[i32] = &[1587];
const ITEM_LIST_632: &[i32] = &[853];
const ITEM_LIST_633: &[i32] = &[1002];
const ITEM_LIST_634: &[i32] = &[1586];
const ITEM_LIST_635: &[i32] = &[1584];
const ITEM_LIST_636: &[i32] = &[75];
const ITEM_LIST_637: &[i32] = &[321];
const ITEM_LIST_638: &[i32] = &[176];
const ITEM_LIST_639: &[i32] = &[176, 189, 201, 214];
const ITEM_LIST_640: &[i32] = &[159];
const ITEM_LIST_641: &[i32] = &[131];
const ITEM_LIST_642: &[i32] = &[143];
const ITEM_LIST_643: &[i32] = &[465];
const ITEM_LIST_644: &[i32] = &[1629];
const ITEM_LIST_645: &[i32] = &[474];
const ITEM_LIST_646: &[i32] = &[1637];
const ITEM_LIST_647: &[i32] = &[902];
const ITEM_LIST_648: &[i32] = &[1645];
const ITEM_LIST_649: &[i32] = &[1621];
const ITEM_LIST_650: &[i32] = &[1517];
const ITEM_LIST_651: &[i32] = &[924];
const ITEM_LIST_652: &[i32] = &[147];
const ITEM_LIST_653: &[i32] = &[160];
const ITEM_LIST_654: &[i32] = &[132];
const ITEM_LIST_655: &[i32] = &[125];
const ITEM_LIST_656: &[i32] = &[144];
const ITEM_LIST_657: &[i32] = &[466];
const ITEM_LIST_658: &[i32] = &[1630];
const ITEM_LIST_659: &[i32] = &[475];
const ITEM_LIST_660: &[i32] = &[1638];
const ITEM_LIST_661: &[i32] = &[903];
const ITEM_LIST_662: &[i32] = &[1646];
const ITEM_LIST_663: &[i32] = &[1622];
const ITEM_LIST_664: &[i32] = &[1518];
const ITEM_LIST_665: &[i32] = &[925];
const ITEM_LIST_666: &[i32] = &[148];
const ITEM_LIST_667: &[i32] = &[841];
const ITEM_LIST_668: &[i32] = &[840];
const ITEM_LIST_669: &[i32] = &[162];
const ITEM_LIST_670: &[i32] = &[134];
const ITEM_LIST_671: &[i32] = &[127];
const ITEM_LIST_672: &[i32] = &[146];
const ITEM_LIST_673: &[i32] = &[468];
const ITEM_LIST_674: &[i32] = &[1632];
const ITEM_LIST_675: &[i32] = &[477];
const ITEM_LIST_676: &[i32] = &[1640];
const ITEM_LIST_677: &[i32] = &[905];
const ITEM_LIST_678: &[i32] = &[1648];
const ITEM_LIST_679: &[i32] = &[1624];
const ITEM_LIST_680: &[i32] = &[1520];
const ITEM_LIST_681: &[i32] = &[927];
const ITEM_LIST_682: &[i32] = &[150];
const ITEM_LIST_683: &[i32] = &[843];
const ITEM_LIST_684: &[i32] = &[161];
const ITEM_LIST_685: &[i32] = &[133];
const ITEM_LIST_686: &[i32] = &[126];
const ITEM_LIST_687: &[i32] = &[145];
const ITEM_LIST_688: &[i32] = &[467];
const ITEM_LIST_689: &[i32] = &[1631];
const ITEM_LIST_690: &[i32] = &[476];
const ITEM_LIST_691: &[i32] = &[1639];
const ITEM_LIST_692: &[i32] = &[904];
const ITEM_LIST_693: &[i32] = &[122];
const ITEM_LIST_694: &[i32] = &[1647];
const ITEM_LIST_695: &[i32] = &[1623];
const ITEM_LIST_696: &[i32] = &[1519];
const ITEM_LIST_697: &[i32] = &[926];
const ITEM_LIST_698: &[i32] = &[141];
const ITEM_LIST_699: &[i32] = &[157];
const ITEM_LIST_700: &[i32] = &[149];
const ITEM_LIST_701: &[i32] = &[842];
const ITEM_LIST_702: &[i32] = &[1591];
const ITEM_LIST_703: &[i32] = &[250];
const ITEM_LIST_704: &[i32] = &[1417];
const ITEM_LIST_705: &[i32] = &[689];
const ITEM_LIST_706: &[i32] = &[282];
const ITEM_LIST_707: &[i32] = &[561];
const ITEM_LIST_708: &[i32] = &[605];
const ITEM_LIST_709: &[i32] = &[1583];
const ITEM_LIST_710: &[i32] = &[1003];
const ITEM_LIST_711: &[i32] = &[1004];
const ITEM_LIST_712: &[i32] = &[254];
const ITEM_LIST_713: &[i32] = &[1421];
const ITEM_LIST_714: &[i32] = &[693];
const ITEM_LIST_715: &[i32] = &[286];
const ITEM_LIST_716: &[i32] = &[599];
const ITEM_LIST_717: &[i32] = &[330];
const ITEM_LIST_718: &[i32] = &[565];
const ITEM_LIST_719: &[i32] = &[609];
const INGREDIENT_0: Ingredient = Ingredient { tag: None, items: ITEM_LIST_0 };
const INGREDIENT_1: Ingredient = Ingredient { tag: None, items: ITEM_LIST_1 };
const INGREDIENT_2: Ingredient = Ingredient { tag: None, items: ITEM_LIST_2 };
const INGREDIENT_3: Ingredient = Ingredient { tag: None, items: ITEM_LIST_3 };
const INGREDIENT_4: Ingredient = Ingredient { tag: None, items: ITEM_LIST_4 };
const INGREDIENT_5: Ingredient = Ingredient { tag: None, items: ITEM_LIST_5 };
const INGREDIENT_6: Ingredient = Ingredient { tag: Some("minecraft:acacia_logs"), items: ITEM_LIST_6 };
const INGREDIENT_7: Ingredient = Ingredient { tag: None, items: ITEM_LIST_7 };
const INGREDIENT_8: Ingredient = Ingredient { tag: None, items: ITEM_LIST_8 };
const INGREDIENT_9: Ingredient = Ingredient { tag: None, items: ITEM_LIST_9 };
const INGREDIENT_10: Ingredient = Ingredient { tag: None, items: ITEM_LIST_10 };
const INGREDIENT_11: Ingredient = Ingredient { tag: None, items: ITEM_LIST_11 };
const INGREDIENT_12: Ingredient = Ingredient { tag: None, items: ITEM_LIST_12 };
const INGREDIENT_13: Ingredient = Ingredient { tag: None, items: ITEM_LIST_13 };
const INGREDIENT_14: Ingredient = Ingredient { tag: None, items: ITEM_LIST_14 };
const INGREDIENT_15: Ingredient = Ingredient { tag: None, items: ITEM_LIST_15 };
const INGREDIENT_16: Ingredient = Ingredient { tag: None, items: ITEM_LIST_16 };
const INGREDIENT_17: Ingredient = Ingredient { tag: None, items: ITEM_LIST_17 };
const INGREDIENT_18: Ingredient = Ingredient { tag: None, items: ITEM_LIST_18 };
const INGREDIENT_19: Ingredient = Ingredient { tag: None, items: ITEM_LIST_19 };
const INGREDIENT_20: Ingredient = Ingredient { tag: None, items: ITEM_LIST_20 };
const INGREDIENT_21: Ingredient = Ingredient { tag: None, items: ITEM_LIST_21 };
const INGREDIENT_22: Ingredient = Ingredient { tag: None, items: ITEM_LIST_22 };
const INGREDIENT_23: Ingredient = Ingredient { tag: None, items: ITEM_LIST_23 };
const INGREDIENT_24: Ingredient = Ingredient { tag: None, items: ITEM_LIST_24 };
const INGREDIENT_25: Ingredient = Ingredient { tag: Some("minecraft:bamboo_blocks"), items: ITEM_LIST_25 };
const INGREDIENT_26: Ingredient = Ingredient { tag: Some("minecraft:planks"), items: ITEM_LIST_26 };
const INGREDIENT_27: Ingredient = Ingredient { tag: Some("minecraft:wooden_slabs"), items: ITEM_LIST_27 };
const INGREDIENT_28: Ingredient = Ingredient { tag: None, items: ITEM_LIST_28 };
const INGREDIENT_29: Ingredient = Ingredient { tag: None, items: ITEM_LIST_29 };
const INGREDIENT_30: Ingredient = Ingredient { tag: None, items: ITEM_LIST_30 };
const INGREDIENT_31: Ingredient = Ingredient { tag: None, items: ITEM_LIST_31 };
const INGREDIENT_32: Ingredient = Ingredient { tag: None, items: ITEM_LIST_32 };
const INGREDIENT_33: Ingredient = Ingredient { tag: None, items: ITEM_LIST_33 };
const INGREDIENT_34: Ingredient = Ingredient { tag: None, items: ITEM_LIST_34 };
const INGREDIENT_35: Ingredient = Ingredient { tag: None, items: ITEM_LIST_35 };
const INGREDIENT_36: Ingredient = Ingredient { tag: None, items: ITEM_LIST_36 };
const INGREDIENT_37: Ingredient = Ingredient { tag: Some("minecraft:birch_logs"), items: ITEM_LIST_37 };
const INGREDIENT_38: Ingredient = Ingredient { tag: None, items: ITEM_LIST_38 };
const INGREDIENT_39: Ingredient = Ingredient { tag: None, items: ITEM_LIST_39 };
const INGREDIENT_40: Ingredient = Ingredient { tag: None, items: ITEM_LIST_40 };
const INGREDIENT_41: Ingredient = Ingredient { tag: Some("minecraft:bundles"), items: ITEM_LIST_41 };
const INGREDIENT_42: Ingredient = Ingredient { tag: None, items: ITEM_LIST_42 };
const INGREDIENT_43: Ingredient = Ingredient { tag: None, items: ITEM_LIST_43 };
const INGREDIENT_44: Ingredient = Ingredient { tag: None, items: ITEM_LIST_44 };
const INGREDIENT_45: Ingredient = Ingredient { tag: None, items: ITEM_LIST_45 };
const INGREDIENT_46: Ingredient = Ingredient { tag: None, items: ITEM_LIST_46 };
const INGREDIENT_47: Ingredient = Ingredient { tag: None, items: ITEM_LIST_47 };
const INGREDIENT_48: Ingredient = Ingredient { tag: None, items: ITEM_LIST_48 };
const INGREDIENT_49: Ingredient = Ingredient { tag: None, items: ITEM_LIST_49 };
const INGREDIENT_50: Ingredient = Ingredient { tag: None, items: ITEM_LIST_50 };
const INGREDIENT_51: Ingredient = Ingredient { tag: None, items: ITEM_LIST_51 };
const INGREDIENT_52: Ingredient = Ingredient { tag: Some("minecraft:shulker_boxes"), items: ITEM_LIST_52 };
const INGREDIENT_53: Ingredient = Ingredient { tag: None, items: ITEM_LIST_53 };
const INGREDIENT_54: Ingredient = Ingredient { tag: None, items: ITEM_LIST_54 };
const INGREDIENT_55: Ingredient = Ingredient { tag: None, items: ITEM_LIST_55 };
const INGREDIENT_56: Ingredient = Ingredient { tag: None, items: ITEM_LIST_56 };
const INGREDIENT_57: Ingredient = Ingredient { tag: None, items: ITEM_LIST_57 };
const INGREDIENT_58: Ingredient = Ingredient { tag: None, items: ITEM_LIST_58 };
const INGREDIENT_59: Ingredient = Ingredient { tag: None, items: ITEM_LIST_59 };
const INGREDIENT_60: Ingredient = Ingredient { tag: None, items: ITEM_LIST_60 };
const INGREDIENT_61: Ingredient = Ingredient { tag: None, items: ITEM_LIST_61 };
const INGREDIENT_62: Ingredient = Ingredient { tag: None, items: ITEM_LIST_62 };
const INGREDIENT_63: Ingredient = Ingredient { tag: None, items: ITEM_LIST_63 };
const INGREDIENT_64: Ingredient = Ingredient { tag: None, items: ITEM_LIST_64 };
const INGREDIENT_65: Ingredient = Ingredient { tag: None, items: ITEM_LIST_65 };
const INGREDIENT_66: Ingredient = Ingredient { tag: None, items: ITEM_LIST_66 };
const INGREDIENT_67: Ingredient = Ingredient { tag: None, items: ITEM_LIST_67 };
const INGREDIENT_68: Ingredient = Ingredient { tag: None, items: ITEM_LIST_68 };
const INGREDIENT_69: Ingredient = Ingredient { tag: None, items: ITEM_LIST_69 };
const INGREDIENT_70: Ingredient = Ingredient { tag: None, items: ITEM_LIST_70 };
const INGREDIENT_71: Ingredient = Ingredient { tag: None, items: ITEM_LIST_71 };
const INGREDIENT_72: Ingredient = Ingredient { tag: None, items: ITEM_LIST_72 };
const INGREDIENT_73: Ingredient = Ingredient { tag: Some("minecraft:trimmable_armor"), items: ITEM_LIST_73 };
const INGREDIENT_74: Ingredient = Ingredient { tag: Some("minecraft:trim_materials"), items: ITEM_LIST_74 };
const INGREDIENT_75: Ingredient = Ingredient { tag: None, items: ITEM_LIST_75 };
const INGREDIENT_76: Ingredient = Ingredient { tag: None, items: ITEM_LIST_76 };
const INGREDIENT_77: Ingredient = Ingredient { tag: None, items: ITEM_LIST_77 };
const INGREDIENT_78: Ingredient = Ingredient { tag: None, items: ITEM_LIST_78 };
const INGREDIENT_79: Ingredient = Ingredient { tag: None, items: ITEM_LIST_79 };
const INGREDIENT_80: Ingredient = Ingredient { tag: Some("minecraft:book_cloning_target"), items: ITEM_LIST_80 };
const INGREDIENT_81: Ingredient = Ingredient { tag: None, items: ITEM_LIST_81 };
const INGREDIENT_82: Ingredient = Ingredient { tag: None, items: ITEM_LIST_82 };
const INGREDIENT_83: Ingredient = Ingredient { tag: None, items: ITEM_LIST_83 };
const INGREDIENT_84: Ingredient = Ingredient { tag: None, items: ITEM_LIST_84 };
const INGREDIENT_85: Ingredient = Ingredient { tag: None, items: ITEM_LIST_85 };
const INGREDIENT_86: Ingredient = Ingredient { tag: None, items: ITEM_LIST_86 };
const INGREDIENT_87: Ingredient = Ingredient { tag: None, items: ITEM_LIST_87 };
const INGREDIENT_88: Ingredient = Ingredient { tag: None, items: ITEM_LIST_88 };
const INGREDIENT_89: Ingredient = Ingredient { tag: None, items: ITEM_LIST_89 };
const INGREDIENT_90: Ingredient = Ingredient { tag: None, items: ITEM_LIST_90 };
const INGREDIENT_91: Ingredient = Ingredient { tag: None, items: ITEM_LIST_91 };
const INGREDIENT_92: Ingredient = Ingredient { tag: None, items: ITEM_LIST_92 };
const INGREDIENT_93: Ingredient = Ingredient { tag: None, items: ITEM_LIST_93 };
const INGREDIENT_94: Ingredient = Ingredient { tag: None, items: ITEM_LIST_94 };
const INGREDIENT_95: Ingredient = Ingredient { tag: None, items: ITEM_LIST_95 };
const INGREDIENT_96: Ingredient = Ingredient { tag: None, items: ITEM_LIST_96 };
const INGREDIENT_97: Ingredient = Ingredient { tag: None, items: ITEM_LIST_97 };
const INGREDIENT_98: Ingredient = Ingredient { tag: None, items: ITEM_LIST_98 };
const INGREDIENT_99: Ingredient = Ingredient { tag: None, items: ITEM_LIST_99 };
const INGREDIENT_100: Ingredient = Ingredient { tag: None, items: ITEM_LIST_100 };
const INGREDIENT_101: Ingredient = Ingredient { tag: None, items: ITEM_LIST_101 };
const INGREDIENT_102: Ingredient = Ingredient { tag: None, items: ITEM_LIST_102 };
const INGREDIENT_103: Ingredient = Ingredient { tag: None, items: ITEM_LIST_103 };
const INGREDIENT_104: Ingredient = Ingredient { tag: None, items: ITEM_LIST_104 };
const INGREDIENT_105: Ingredient = Ingredient { tag: None, items: ITEM_LIST_105 };
const INGREDIENT_106: Ingredient = Ingredient { tag: None, items: ITEM_LIST_106 };
const INGREDIENT_107: Ingredient = Ingredient { tag: None, items: ITEM_LIST_107 };
const INGREDIENT_108: Ingredient = Ingredient { tag: None, items: ITEM_LIST_108 };
const INGREDIENT_109: Ingredient = Ingredient { tag: Some("minecraft:stone_crafting_materials"), items: ITEM_LIST_109 };
const INGREDIENT_110: Ingredient = Ingredient { tag: None, items: ITEM_LIST_110 };
const INGREDIENT_111: Ingredient = Ingredient { tag: None, items: ITEM_LIST_111 };
const INGREDIENT_112: Ingredient = Ingredient { tag: None, items: ITEM_LIST_112 };
const INGREDIENT_113: Ingredient = Ingredient { tag: None, items: ITEM_LIST_113 };
const INGREDIENT_114: Ingredient = Ingredient { tag: None, items: ITEM_LIST_114 };
const INGREDIENT_115: Ingredient = Ingredient { tag: None, items: ITEM_LIST_115 };
const INGREDIENT_116: Ingredient = Ingredient { tag: None, items: ITEM_LIST_116 };
const INGREDIENT_117: Ingredient = Ingredient { tag: None, items: ITEM_LIST_117 };
const INGREDIENT_118: Ingredient = Ingredient { tag: None, items: ITEM_LIST_118 };
const INGREDIENT_119: Ingredient = Ingredient { tag: None, items: ITEM_LIST_119 };
const INGREDIENT_120: Ingredient = Ingredient { tag: None, items: ITEM_LIST_120 };
const INGREDIENT_121: Ingredient = Ingredient { tag: None, items: ITEM_LIST_121 };
const INGREDIENT_122: Ingredient = Ingredient { tag: None, items: ITEM_LIST_122 };
const INGREDIENT_123: Ingredient = Ingredient { tag: Some("minecraft:eggs"), items: ITEM_LIST_123 };
const INGREDIENT_124: Ingredient = Ingredient { tag: None, items: ITEM_LIST_124 };
const INGREDIENT_125: Ingredient = Ingredient { tag: Some("minecraft:coals"), items: ITEM_LIST_125 };
const INGREDIENT_126: Ingredient = Ingredient { tag: Some("minecraft:logs"), items: ITEM_LIST_126 };
const INGREDIENT_127: Ingredient = Ingredient { tag: None, items: ITEM_LIST_127 };
const INGREDIENT_128: Ingredient = Ingredient { tag: None, items: ITEM_LIST_128 };
const INGREDIENT_129: Ingredient = Ingredient { tag: Some("minecraft:logs_that_burn"), items: ITEM_LIST_129 };
const INGREDIENT_130: Ingredient = Ingredient { tag: None, items: ITEM_LIST_130 };
const INGREDIENT_131: Ingredient = Ingredient { tag: None, items: ITEM_LIST_131 };
const INGREDIENT_132: Ingredient = Ingredient { tag: None, items: ITEM_LIST_132 };
const INGREDIENT_133: Ingredient = Ingredient { tag: Some("minecraft:cherry_logs"), items: ITEM_LIST_133 };
const INGREDIENT_134: Ingredient = Ingredient { tag: None, items: ITEM_LIST_134 };
const INGREDIENT_135: Ingredient = Ingredient { tag: None, items: ITEM_LIST_135 };
const INGREDIENT_136: Ingredient = Ingredient { tag: None, items: ITEM_LIST_136 };
const INGREDIENT_137: Ingredient = Ingredient { tag: None, items: ITEM_LIST_137 };
const INGREDIENT_138: Ingredient = Ingredient { tag: None, items: ITEM_LIST_138 };
const INGREDIENT_139: Ingredient = Ingredient { tag: None, items: ITEM_LIST_139 };
const INGREDIENT_140: Ingredient = Ingredient { tag: None, items: ITEM_LIST_140 };
const INGREDIENT_141: Ingredient = Ingredient { tag: None, items: ITEM_LIST_141 };
const INGREDIENT_142: Ingredient = Ingredient { tag: None, items: ITEM_LIST_142 };
const INGREDIENT_143: Ingredient = Ingredient { tag: None, items: ITEM_LIST_143 };
const INGREDIENT_144: Ingredient = Ingredient { tag: None, items: ITEM_LIST_144 };
const INGREDIENT_145: Ingredient = Ingredient { tag: None, items: ITEM_LIST_145 };
const INGREDIENT_146: Ingredient = Ingredient { tag: None, items: ITEM_LIST_146 };
const INGREDIENT_147: Ingredient = Ingredient { tag: None, items: ITEM_LIST_147 };
const INGREDIENT_148: Ingredient = Ingredient { tag: None, items: ITEM_LIST_148 };
const INGREDIENT_149: Ingredient = Ingredient { tag: None, items: ITEM_LIST_149 };
const INGREDIENT_150: Ingredient = Ingredient { tag: None, items: ITEM_LIST_150 };
const INGREDIENT_151: Ingredient = Ingredient { tag: None, items: ITEM_LIST_151 };
const INGREDIENT_152: Ingredient = Ingredient { tag: None, items: ITEM_LIST_152 };
const INGREDIENT_153: Ingredient = Ingredient { tag: None, items: ITEM_LIST_153 };
const INGREDIENT_154: Ingredient = Ingredient { tag: None, items: ITEM_LIST_154 };
const INGREDIENT_155: Ingredient = Ingredient { tag: None, items: ITEM_LIST_155 };
const INGREDIENT_156: Ingredient = Ingredient { tag: None, items: ITEM_LIST_156 };
const INGREDIENT_157: Ingredient = Ingredient { tag: None, items: ITEM_LIST_157 };
const INGREDIENT_158: Ingredient = Ingredient { tag: None, items: ITEM_LIST_158 };
const INGREDIENT_159: Ingredient = Ingredient { tag: None, items: ITEM_LIST_159 };
const INGREDIENT_160: Ingredient = Ingredient { tag: None, items: ITEM_LIST_160 };
const INGREDIENT_161: Ingredient = Ingredient { tag: None, items: ITEM_LIST_161 };
const INGREDIENT_162: Ingredient = Ingredient { tag: None, items: ITEM_LIST_162 };
const INGREDIENT_163: Ingredient = Ingredient { tag: None, items: ITEM_LIST_163 };
const INGREDIENT_164: Ingredient = Ingredient { tag: None, items: ITEM_LIST_164 };
const INGREDIENT_165: Ingredient = Ingredient { tag: None, items: ITEM_LIST_165 };
const INGREDIENT_166: Ingredient = Ingredient { tag: None, items: ITEM_LIST_166 };
const INGREDIENT_167: Ingredient = Ingredient { tag: None, items: ITEM_LIST_167 };
const INGREDIENT_168: Ingredient = Ingredient { tag: None, items: ITEM_LIST_168 };
const INGREDIENT_169: Ingredient = Ingredient { tag: None, items: ITEM_LIST_169 };
const INGREDIENT_170: Ingredient = Ingredient { tag: None, items: ITEM_LIST_170 };
const INGREDIENT_171: Ingredient = Ingredient { tag: None, items: ITEM_LIST_171 };
const INGREDIENT_172: Ingredient = Ingredient { tag: None, items: ITEM_LIST_172 };
const INGREDIENT_173: Ingredient = Ingredient { tag: None, items: ITEM_LIST_173 };
const INGREDIENT_174: Ingredient = Ingredient { tag: None, items: ITEM_LIST_174 };
const INGREDIENT_175: Ingredient = Ingredient { tag: None, items: ITEM_LIST_175 };
const INGREDIENT_176: Ingredient = Ingredient { tag: None, items: ITEM_LIST_176 };
const INGREDIENT_177: Ingredient = Ingredient { tag: None, items: ITEM_LIST_177 };
const INGREDIENT_178: Ingredient = Ingredient { tag: None, items: ITEM_LIST_178 };
const INGREDIENT_179: Ingredient = Ingredient { tag: None, items: ITEM_LIST_179 };
const INGREDIENT_180: Ingredient = Ingredient { tag: None, items: ITEM_LIST_180 };
const INGREDIENT_181: Ingredient = Ingredient { tag: None, items: ITEM_LIST_181 };
const INGREDIENT_182: Ingredient = Ingredient { tag: None, items: ITEM_LIST_182 };
const INGREDIENT_183: Ingredient = Ingredient { tag: None, items: ITEM_LIST_183 };
const INGREDIENT_184: Ingredient = Ingredient { tag: Some("minecraft:copper_tool_materials"), items: ITEM_LIST_121 };
const INGREDIENT_185: Ingredient = Ingredient { tag: None, items: ITEM_LIST_184 };
const INGREDIENT_186: Ingredient = Ingredient { tag: None, items: ITEM_LIST_185 };
const INGREDIENT_187: Ingredient = Ingredient { tag: None, items: ITEM_LIST_186 };
const INGREDIENT_188: Ingredient = Ingredient { tag: None, items: ITEM_LIST_187 };
const INGREDIENT_189: Ingredient = Ingredient { tag: None, items: ITEM_LIST_188 };
const INGREDIENT_190: Ingredient = Ingredient { tag: None, items: ITEM_LIST_189 };
const INGREDIENT_191: Ingredient = Ingredient { tag: None, items: ITEM_LIST_190 };
const INGREDIENT_192: Ingredient = Ingredient { tag: None, items: ITEM_LIST_125 };
const INGREDIENT_193: Ingredient = Ingredient { tag: None, items: ITEM_LIST_191 };
const INGREDIENT_194: Ingredient = Ingredient { tag: None, items: ITEM_LIST_192 };
const INGREDIENT_195: Ingredient = Ingredient { tag: None, items: ITEM_LIST_193 };
const INGREDIENT_196: Ingredient = Ingredient { tag: None, items: ITEM_LIST_194 };
const INGREDIENT_197: Ingredient = Ingredient { tag: None, items: ITEM_LIST_195 };
const INGREDIENT_198: Ingredient = Ingredient { tag: None, items: ITEM_LIST_196 };
const INGREDIENT_199: Ingredient = Ingredient { tag: None, items: ITEM_LIST_197 };
const INGREDIENT_200: Ingredient = Ingredient { tag: None, items: ITEM_LIST_198 };
const INGREDIENT_201: Ingredient = Ingredient { tag: None, items: ITEM_LIST_199 };
const INGREDIENT_202: Ingredient = Ingredient { tag: None, items: ITEM_LIST_200 };
const INGREDIENT_203: Ingredient = Ingredient { tag: None, items: ITEM_LIST_201 };
const INGREDIENT_204: Ingredient = Ingredient { tag: Some("minecraft:crimson_stems"), items: ITEM_LIST_202 };
const INGREDIENT_205: Ingredient = Ingredient { tag: None, items: ITEM_LIST_203 };
const INGREDIENT_206: Ingredient = Ingredient { tag: None, items: ITEM_LIST_204 };
const INGREDIENT_207: Ingredient = Ingredient { tag: None, items: ITEM_LIST_205 };
const INGREDIENT_208: Ingredient = Ingredient { tag: None, items: ITEM_LIST_206 };
const INGREDIENT_209: Ingredient = Ingredient { tag: None, items: ITEM_LIST_207 };
const INGREDIENT_210: Ingredient = Ingredient { tag: None, items: ITEM_LIST_208 };
const INGREDIENT_211: Ingredient = Ingredient { tag: None, items: ITEM_LIST_209 };
const INGREDIENT_212: Ingredient = Ingredient { tag: None, items: ITEM_LIST_210 };
const INGREDIENT_213: Ingredient = Ingredient { tag: None, items: ITEM_LIST_211 };
const INGREDIENT_214: Ingredient = Ingredient { tag: None, items: ITEM_LIST_212 };
const INGREDIENT_215: Ingredient = Ingredient { tag: None, items: ITEM_LIST_213 };
const INGREDIENT_216: Ingredient = Ingredient { tag: None, items: ITEM_LIST_214 };
const INGREDIENT_217: Ingredient = Ingredient { tag: None, items: ITEM_LIST_215 };
const INGREDIENT_218: Ingredient = Ingredient { tag: None, items: ITEM_LIST_216 };
const INGREDIENT_219: Ingredient = Ingredient { tag: None, items: ITEM_LIST_217 };
const INGREDIENT_220: Ingredient = Ingredient { tag: Some("minecraft:dark_oak_logs"), items: ITEM_LIST_218 };
const INGREDIENT_221: Ingredient = Ingredient { tag: None, items: ITEM_LIST_219 };
const INGREDIENT_222: Ingredient = Ingredient { tag: None, items: ITEM_LIST_220 };
const INGREDIENT_223: Ingredient = Ingredient { tag: None, items: ITEM_LIST_221 };
const INGREDIENT_224: Ingredient = Ingredient { tag: Some("minecraft:decorated_pot_ingredients"), items: ITEM_LIST_222 };
const INGREDIENT_225: Ingredient = Ingredient { tag: None, items: ITEM_LIST_223 };
const INGREDIENT_226: Ingredient = Ingredient { tag: None, items: ITEM_LIST_224 };
const INGREDIENT_227: Ingredient = Ingredient { tag: None, items: ITEM_LIST_225 };
const INGREDIENT_228: Ingredient = Ingredient { tag: Some("minecraft:diamond_tool_materials"), items: ITEM_LIST_70 };
const INGREDIENT_229: Ingredient = Ingredient { tag: None, items: ITEM_LIST_226 };
const INGREDIENT_230: Ingredient = Ingredient { tag: None, items: ITEM_LIST_227 };
const INGREDIENT_231: Ingredient = Ingredient { tag: None, items: ITEM_LIST_228 };
const INGREDIENT_232: Ingredient = Ingredient { tag: None, items: ITEM_LIST_229 };
const INGREDIENT_233: Ingredient = Ingredient { tag: None, items: ITEM_LIST_230 };
const INGREDIENT_234: Ingredient = Ingredient { tag: None, items: ITEM_LIST_231 };
const INGREDIENT_235: Ingredient = Ingredient { tag: None, items: ITEM_LIST_232 };
const INGREDIENT_236: Ingredient = Ingredient { tag: None, items: ITEM_LIST_233 };
const INGREDIENT_237: Ingredient = Ingredient { tag: None, items: ITEM_LIST_234 };
const INGREDIENT_238: Ingredient = Ingredient { tag: None, items: ITEM_LIST_235 };
const INGREDIENT_239: Ingredient = Ingredient { tag: None, items: ITEM_LIST_236 };
const INGREDIENT_240: Ingredient = Ingredient { tag: None, items: ITEM_LIST_237 };
const INGREDIENT_241: Ingredient = Ingredient { tag: None, items: ITEM_LIST_238 };
const INGREDIENT_242: Ingredient = Ingredient { tag: None, items: ITEM_LIST_239 };
const INGREDIENT_243: Ingredient = Ingredient { tag: None, items: ITEM_LIST_240 };
const INGREDIENT_244: Ingredient = Ingredient { tag: None, items: ITEM_LIST_241 };
const INGREDIENT_245: Ingredient = Ingredient { tag: None, items: ITEM_LIST_242 };
const INGREDIENT_246: Ingredient = Ingredient { tag: None, items: ITEM_LIST_243 };
const INGREDIENT_247: Ingredient = Ingredient { tag: None, items: ITEM_LIST_244 };
const INGREDIENT_248: Ingredient = Ingredient { tag: None, items: ITEM_LIST_245 };
const INGREDIENT_249: Ingredient = Ingredient { tag: None, items: ITEM_LIST_246 };
const INGREDIENT_250: Ingredient = Ingredient { tag: None, items: ITEM_LIST_247 };
const INGREDIENT_251: Ingredient = Ingredient { tag: None, items: ITEM_LIST_248 };
const INGREDIENT_252: Ingredient = Ingredient { tag: None, items: ITEM_LIST_249 };
const INGREDIENT_253: Ingredient = Ingredient { tag: None, items: ITEM_LIST_250 };
const INGREDIENT_254: Ingredient = Ingredient { tag: None, items: ITEM_LIST_251 };
const INGREDIENT_255: Ingredient = Ingredient { tag: None, items: ITEM_LIST_252 };
const INGREDIENT_256: Ingredient = Ingredient { tag: None, items: ITEM_LIST_253 };
const INGREDIENT_257: Ingredient = Ingredient { tag: None, items: ITEM_LIST_254 };
const INGREDIENT_258: Ingredient = Ingredient { tag: None, items: ITEM_LIST_255 };
const INGREDIENT_259: Ingredient = Ingredient { tag: None, items: ITEM_LIST_256 };
const INGREDIENT_260: Ingredient = Ingredient { tag: None, items: ITEM_LIST_257 };
const INGREDIENT_261: Ingredient = Ingredient { tag: None, items: ITEM_LIST_258 };
const INGREDIENT_262: Ingredient = Ingredient { tag: None, items: ITEM_LIST_259 };
const INGREDIENT_263: Ingredient = Ingredient { tag: None, items: ITEM_LIST_260 };
const INGREDIENT_264: Ingredient = Ingredient { tag: None, items: ITEM_LIST_261 };
const INGREDIENT_265: Ingredient = Ingredient { tag: None, items: ITEM_LIST_262 };
const INGREDIENT_266: Ingredient = Ingredient { tag: None, items: ITEM_LIST_263 };
const INGREDIENT_267: Ingredient = Ingredient { tag: None, items: ITEM_LIST_264 };
const INGREDIENT_268: Ingredient = Ingredient { tag: None, items: ITEM_LIST_265 };
const INGREDIENT_269: Ingredient = Ingredient { tag: None, items: ITEM_LIST_266 };
const INGREDIENT_270: Ingredient = Ingredient { tag: None, items: ITEM_LIST_267 };
const INGREDIENT_271: Ingredient = Ingredient { tag: None, items: ITEM_LIST_268 };
const INGREDIENT_272: Ingredient = Ingredient { tag: None, items: ITEM_LIST_269 };
const INGREDIENT_273: Ingredient = Ingredient { tag: None, items: ITEM_LIST_270 };
const INGREDIENT_274: Ingredient = Ingredient { tag: None, items: ITEM_LIST_271 };
const INGREDIENT_275: Ingredient = Ingredient { tag: None, items: ITEM_LIST_272 };
const INGREDIENT_276: Ingredient = Ingredient { tag: None, items: ITEM_LIST_273 };
const INGREDIENT_277: Ingredient = Ingredient { tag: None, items: ITEM_LIST_274 };
const INGREDIENT_278: Ingredient = Ingredient { tag: None, items: ITEM_LIST_275 };
const INGREDIENT_279: Ingredient = Ingredient { tag: None, items: ITEM_LIST_276 };
const INGREDIENT_280: Ingredient = Ingredient { tag: None, items: ITEM_LIST_277 };
const INGREDIENT_281: Ingredient = Ingredient { tag: None, items: ITEM_LIST_278 };
const INGREDIENT_282: Ingredient = Ingredient { tag: None, items: ITEM_LIST_279 };
const INGREDIENT_283: Ingredient = Ingredient { tag: None, items: ITEM_LIST_280 };
const INGREDIENT_284: Ingredient = Ingredient { tag: None, items: ITEM_LIST_281 };
const INGREDIENT_285: Ingredient = Ingredient { tag: None, items: ITEM_LIST_282 };
const INGREDIENT_286: Ingredient = Ingredient { tag: None, items: ITEM_LIST_283 };
const INGREDIENT_287: Ingredient = Ingredient { tag: None, items: ITEM_LIST_284 };
const INGREDIENT_288: Ingredient = Ingredient { tag: None, items: ITEM_LIST_285 };
const INGREDIENT_289: Ingredient = Ingredient { tag: None, items: ITEM_LIST_286 };
const INGREDIENT_290: Ingredient = Ingredient { tag: None, items: ITEM_LIST_287 };
const INGREDIENT_291: Ingredient = Ingredient { tag: None, items: ITEM_LIST_288 };
const INGREDIENT_292: Ingredient = Ingredient { tag: None, items: ITEM_LIST_289 };
const INGREDIENT_293: Ingredient = Ingredient { tag: None, items: ITEM_LIST_290 };
const INGREDIENT_294: Ingredient = Ingredient { tag: None, items: ITEM_LIST_291 };
const INGREDIENT_295: Ingredient = Ingredient { tag: None, items: ITEM_LIST_292 };
const INGREDIENT_296: Ingredient = Ingredient { tag: None, items: ITEM_LIST_293 };
const INGREDIENT_297: Ingredient = Ingredient { tag: None, items: ITEM_LIST_294 };
const INGREDIENT_298: Ingredient = Ingredient { tag: None, items: ITEM_LIST_295 };
const INGREDIENT_299: Ingredient = Ingredient { tag: None, items: ITEM_LIST_296 };
const INGREDIENT_300: Ingredient = Ingredient { tag: None, items: ITEM_LIST_297 };
const INGREDIENT_301: Ingredient = Ingredient { tag: None, items: ITEM_LIST_298 };
const INGREDIENT_302: Ingredient = Ingredient { tag: None, items: ITEM_LIST_299 };
const INGREDIENT_303: Ingredient = Ingredient { tag: None, items: ITEM_LIST_300 };
const INGREDIENT_304: Ingredient = Ingredient { tag: None, items: ITEM_LIST_301 };
const INGREDIENT_305: Ingredient = Ingredient { tag: None, items: ITEM_LIST_302 };
const INGREDIENT_306: Ingredient = Ingredient { tag: None, items: ITEM_LIST_303 };
const INGREDIENT_307: Ingredient = Ingredient { tag: None, items: ITEM_LIST_304 };
const INGREDIENT_308: Ingredient = Ingredient { tag: None, items: ITEM_LIST_305 };
const INGREDIENT_309: Ingredient = Ingredient { tag: None, items: ITEM_LIST_306 };
const INGREDIENT_310: Ingredient = Ingredient { tag: None, items: ITEM_LIST_307 };
const INGREDIENT_311: Ingredient = Ingredient { tag: None, items: ITEM_LIST_308 };
const INGREDIENT_312: Ingredient = Ingredient { tag: None, items: ITEM_LIST_309 };
const INGREDIENT_313: Ingredient = Ingredient { tag: None, items: ITEM_LIST_310 };
const INGREDIENT_314: Ingredient = Ingredient { tag: None, items: ITEM_LIST_311 };
const INGREDIENT_315: Ingredient = Ingredient { tag: None, items: ITEM_LIST_312 };
const INGREDIENT_316: Ingredient = Ingredient { tag: None, items: ITEM_LIST_313 };
const INGREDIENT_317: Ingredient = Ingredient { tag: None, items: ITEM_LIST_314 };
const INGREDIENT_318: Ingredient = Ingredient { tag: None, items: ITEM_LIST_315 };
const INGREDIENT_319: Ingredient = Ingredient { tag: None, items: ITEM_LIST_316 };
const INGREDIENT_320: Ingredient = Ingredient { tag: None, items: ITEM_LIST_317 };
const INGREDIENT_321: Ingredient = Ingredient { tag: None, items: ITEM_LIST_318 };
const INGREDIENT_322: Ingredient = Ingredient { tag: None, items: ITEM_LIST_319 };
const INGREDIENT_323: Ingredient = Ingredient { tag: None, items: ITEM_LIST_320 };
const INGREDIENT_324: Ingredient = Ingredient { tag: None, items: ITEM_LIST_321 };
const INGREDIENT_325: Ingredient = Ingredient { tag: None, items: ITEM_LIST_322 };
const INGREDIENT_326: Ingredient = Ingredient { tag: None, items: ITEM_LIST_323 };
const INGREDIENT_327: Ingredient = Ingredient { tag: None, items: ITEM_LIST_324 };
const INGREDIENT_328: Ingredient = Ingredient { tag: None, items: ITEM_LIST_325 };
const INGREDIENT_329: Ingredient = Ingredient { tag: None, items: ITEM_LIST_326 };
const INGREDIENT_330: Ingredient = Ingredient { tag: None, items: ITEM_LIST_327 };
const INGREDIENT_331: Ingredient = Ingredient { tag: None, items: ITEM_LIST_328 };
const INGREDIENT_332: Ingredient = Ingredient { tag: None, items: ITEM_LIST_329 };
const INGREDIENT_333: Ingredient = Ingredient { tag: None, items: ITEM_LIST_330 };
const INGREDIENT_334: Ingredient = Ingredient { tag: None, items: ITEM_LIST_331 };
const INGREDIENT_335: Ingredient = Ingredient { tag: None, items: ITEM_LIST_332 };
const INGREDIENT_336: Ingredient = Ingredient { tag: None, items: ITEM_LIST_333 };
const INGREDIENT_337: Ingredient = Ingredient { tag: None, items: ITEM_LIST_334 };
const INGREDIENT_338: Ingredient = Ingredient { tag: None, items: ITEM_LIST_335 };
const INGREDIENT_339: Ingredient = Ingredient { tag: None, items: ITEM_LIST_336 };
const INGREDIENT_340: Ingredient = Ingredient { tag: None, items: ITEM_LIST_337 };
const INGREDIENT_341: Ingredient = Ingredient { tag: None, items: ITEM_LIST_338 };
const INGREDIENT_342: Ingredient = Ingredient { tag: None, items: ITEM_LIST_339 };
const INGREDIENT_343: Ingredient = Ingredient { tag: None, items: ITEM_LIST_340 };
const INGREDIENT_344: Ingredient = Ingredient { tag: None, items: ITEM_LIST_341 };
const INGREDIENT_345: Ingredient = Ingredient { tag: None, items: ITEM_LIST_342 };
const INGREDIENT_346: Ingredient = Ingredient { tag: None, items: ITEM_LIST_343 };
const INGREDIENT_347: Ingredient = Ingredient { tag: None, items: ITEM_LIST_344 };
const INGREDIENT_348: Ingredient = Ingredient { tag: None, items: ITEM_LIST_345 };
const INGREDIENT_349: Ingredient = Ingredient { tag: None, items: ITEM_LIST_346 };
const INGREDIENT_350: Ingredient = Ingredient { tag: None, items: ITEM_LIST_347 };
const INGREDIENT_351: Ingredient = Ingredient { tag: None, items: ITEM_LIST_348 };
const INGREDIENT_352: Ingredient = Ingredient { tag: None, items: ITEM_LIST_349 };
const INGREDIENT_353: Ingredient = Ingredient { tag: None, items: ITEM_LIST_350 };
const INGREDIENT_354: Ingredient = Ingredient { tag: None, items: ITEM_LIST_351 };
const INGREDIENT_355: Ingredient = Ingredient { tag: None, items: ITEM_LIST_352 };
const INGREDIENT_356: Ingredient = Ingredient { tag: None, items: ITEM_LIST_353 };
const INGREDIENT_357: Ingredient = Ingredient { tag: None, items: ITEM_LIST_354 };
const INGREDIENT_358: Ingredient = Ingredient { tag: None, items: ITEM_LIST_355 };
const INGREDIENT_359: Ingredient = Ingredient { tag: None, items: ITEM_LIST_356 };
const INGREDIENT_360: Ingredient = Ingredient { tag: None, items: ITEM_LIST_357 };
const INGREDIENT_361: Ingredient = Ingredient { tag: None, items: ITEM_LIST_358 };
const INGREDIENT_362: Ingredient = Ingredient { tag: None, items: ITEM_LIST_359 };
const INGREDIENT_363: Ingredient = Ingredient { tag: None, items: ITEM_LIST_360 };
const INGREDIENT_364: Ingredient = Ingredient { tag: None, items: ITEM_LIST_361 };
const INGREDIENT_365: Ingredient = Ingredient { tag: None, items: ITEM_LIST_362 };
const INGREDIENT_366: Ingredient = Ingredient { tag: None, items: ITEM_LIST_363 };
const INGREDIENT_367: Ingredient = Ingredient { tag: None, items: ITEM_LIST_364 };
const INGREDIENT_368: Ingredient = Ingredient { tag: None, items: ITEM_LIST_365 };
const INGREDIENT_369: Ingredient = Ingredient { tag: None, items: ITEM_LIST_366 };
const INGREDIENT_370: Ingredient = Ingredient { tag: None, items: ITEM_LIST_367 };
const INGREDIENT_371: Ingredient = Ingredient { tag: None, items: ITEM_LIST_368 };
const INGREDIENT_372: Ingredient = Ingredient { tag: None, items: ITEM_LIST_369 };
const INGREDIENT_373: Ingredient = Ingredient { tag: None, items: ITEM_LIST_370 };
const INGREDIENT_374: Ingredient = Ingredient { tag: None, items: ITEM_LIST_371 };
const INGREDIENT_375: Ingredient = Ingredient { tag: None, items: ITEM_LIST_372 };
const INGREDIENT_376: Ingredient = Ingredient { tag: Some("minecraft:skulls"), items: ITEM_LIST_373 };
const INGREDIENT_377: Ingredient = Ingredient { tag: None, items: ITEM_LIST_374 };
const INGREDIENT_378: Ingredient = Ingredient { tag: None, items: ITEM_LIST_375 };
const INGREDIENT_379: Ingredient = Ingredient { tag: Some("minecraft:dyes"), items: ITEM_LIST_376 };
const INGREDIENT_380: Ingredient = Ingredient { tag: None, items: ITEM_LIST_377 };
const INGREDIENT_381: Ingredient = Ingredient { tag: None, items: ITEM_LIST_378 };
const INGREDIENT_382: Ingredient = Ingredient { tag: Some("minecraft:smelts_to_glass"), items: ITEM_LIST_379 };
const INGREDIENT_383: Ingredient = Ingredient { tag: None, items: ITEM_LIST_380 };
const INGREDIENT_384: Ingredient = Ingredient { tag: None, items: ITEM_LIST_381 };
const INGREDIENT_385: Ingredient = Ingredient { tag: None, items: ITEM_LIST_382 };
const INGREDIENT_386: Ingredient = Ingredient { tag: None, items: ITEM_LIST_383 };
const INGREDIENT_387: Ingredient = Ingredient { tag: None, items: ITEM_LIST_384 };
const INGREDIENT_388: Ingredient = Ingredient { tag: None, items: ITEM_LIST_385 };
const INGREDIENT_389: Ingredient = Ingredient { tag: None, items: ITEM_LIST_386 };
const INGREDIENT_390: Ingredient = Ingredient { tag: None, items: ITEM_LIST_387 };
const INGREDIENT_391: Ingredient = Ingredient { tag: None, items: ITEM_LIST_388 };
const INGREDIENT_392: Ingredient = Ingredient { tag: None, items: ITEM_LIST_389 };
const INGREDIENT_393: Ingredient = Ingredient { tag: Some("minecraft:gold_tool_materials"), items: ITEM_LIST_167 };
const INGREDIENT_394: Ingredient = Ingredient { tag: None, items: ITEM_LIST_390 };
const INGREDIENT_395: Ingredient = Ingredient { tag: None, items: ITEM_LIST_391 };
const INGREDIENT_396: Ingredient = Ingredient { tag: None, items: ITEM_LIST_392 };
const INGREDIENT_397: Ingredient = Ingredient { tag: None, items: ITEM_LIST_393 };
const INGREDIENT_398: Ingredient = Ingredient { tag: None, items: ITEM_LIST_394 };
const INGREDIENT_399: Ingredient = Ingredient { tag: None, items: ITEM_LIST_395 };
const INGREDIENT_400: Ingredient = Ingredient { tag: None, items: ITEM_LIST_396 };
const INGREDIENT_401: Ingredient = Ingredient { tag: None, items: ITEM_LIST_397 };
const INGREDIENT_402: Ingredient = Ingredient { tag: None, items: ITEM_LIST_398 };
const INGREDIENT_403: Ingredient = Ingredient { tag: None, items: ITEM_LIST_399 };
const INGREDIENT_404: Ingredient = Ingredient { tag: None, items: ITEM_LIST_400 };
const INGREDIENT_405: Ingredient = Ingredient { tag: None, items: ITEM_LIST_401 };
const INGREDIENT_406: Ingredient = Ingredient { tag: None, items: ITEM_LIST_402 };
const INGREDIENT_407: Ingredient = Ingredient { tag: None, items: ITEM_LIST_403 };
const INGREDIENT_408: Ingredient = Ingredient { tag: None, items: ITEM_LIST_404 };
const INGREDIENT_409: Ingredient = Ingredient { tag: None, items: ITEM_LIST_405 };
const INGREDIENT_410: Ingredient = Ingredient { tag: None, items: ITEM_LIST_406 };
const INGREDIENT_411: Ingredient = Ingredient { tag: None, items: ITEM_LIST_407 };
const INGREDIENT_412: Ingredient = Ingredient { tag: None, items: ITEM_LIST_408 };
const INGREDIENT_413: Ingredient = Ingredient { tag: None, items: ITEM_LIST_409 };
const INGREDIENT_414: Ingredient = Ingredient { tag: None, items: ITEM_LIST_410 };
const INGREDIENT_415: Ingredient = Ingredient { tag: None, items: ITEM_LIST_411 };
const INGREDIENT_416: Ingredient = Ingredient { tag: Some("minecraft:iron_tool_materials"), items: ITEM_LIST_8 };
const INGREDIENT_417: Ingredient = Ingredient { tag: None, items: ITEM_LIST_412 };
const INGREDIENT_418: Ingredient = Ingredient { tag: None, items: ITEM_LIST_413 };
const INGREDIENT_419: Ingredient = Ingredient { tag: None, items: ITEM_LIST_414 };
const INGREDIENT_420: Ingredient = Ingredient { tag: None, items: ITEM_LIST_415 };
const INGREDIENT_421: Ingredient = Ingredient { tag: None, items: ITEM_LIST_416 };
const INGREDIENT_422: Ingredient = Ingredient { tag: None, items: ITEM_LIST_417 };
const INGREDIENT_423: Ingredient = Ingredient { tag: None, items: ITEM_LIST_418 };
const INGREDIENT_424: Ingredient = Ingredient { tag: None, items: ITEM_LIST_419 };
const INGREDIENT_425: Ingredient = Ingredient { tag: None, items: ITEM_LIST_420 };
const INGREDIENT_426: Ingredient = Ingredient { tag: None, items: ITEM_LIST_421 };
const INGREDIENT_427: Ingredient = Ingredient { tag: Some("minecraft:jungle_logs"), items: ITEM_LIST_422 };
const INGREDIENT_428: Ingredient = Ingredient { tag: None, items: ITEM_LIST_423 };
const INGREDIENT_429: Ingredient = Ingredient { tag: None, items: ITEM_LIST_424 };
const INGREDIENT_430: Ingredient = Ingredient { tag: None, items: ITEM_LIST_425 };
const INGREDIENT_431: Ingredient = Ingredient { tag: None, items: ITEM_LIST_426 };
const INGREDIENT_432: Ingredient = Ingredient { tag: Some("minecraft:leaves"), items: ITEM_LIST_427 };
const INGREDIENT_433: Ingredient = Ingredient { tag: None, items: ITEM_LIST_428 };
const INGREDIENT_434: Ingredient = Ingredient { tag: None, items: ITEM_LIST_429 };
const INGREDIENT_435: Ingredient = Ingredient { tag: None, items: ITEM_LIST_430 };
const INGREDIENT_436: Ingredient = Ingredient { tag: None, items: ITEM_LIST_431 };
const INGREDIENT_437: Ingredient = Ingredient { tag: None, items: ITEM_LIST_432 };
const INGREDIENT_438: Ingredient = Ingredient { tag: None, items: ITEM_LIST_433 };
const INGREDIENT_439: Ingredient = Ingredient { tag: None, items: ITEM_LIST_434 };
const INGREDIENT_440: Ingredient = Ingredient { tag: None, items: ITEM_LIST_435 };
const INGREDIENT_441: Ingredient = Ingredient { tag: None, items: ITEM_LIST_436 };
const INGREDIENT_442: Ingredient = Ingredient { tag: None, items: ITEM_LIST_437 };
const INGREDIENT_443: Ingredient = Ingredient { tag: None, items: ITEM_LIST_438 };
const INGREDIENT_444: Ingredient = Ingredient { tag: None, items: ITEM_LIST_439 };
const INGREDIENT_445: Ingredient = Ingredient { tag: None, items: ITEM_LIST_440 };
const INGREDIENT_446: Ingredient = Ingredient { tag: None, items: ITEM_LIST_441 };
const INGREDIENT_447: Ingredient = Ingredient { tag: None, items: ITEM_LIST_442 };
const INGREDIENT_448: Ingredient = Ingredient { tag: None, items: ITEM_LIST_443 };
const INGREDIENT_449: Ingredient = Ingredient { tag: None, items: ITEM_LIST_444 };
const INGREDIENT_450: Ingredient = Ingredient { tag: None, items: ITEM_LIST_445 };
const INGREDIENT_451: Ingredient = Ingredient { tag: None, items: ITEM_LIST_446 };
const INGREDIENT_452: Ingredient = Ingredient { tag: None, items: ITEM_LIST_447 };
const INGREDIENT_453: Ingredient = Ingredient { tag: None, items: ITEM_LIST_448 };
const INGREDIENT_454: Ingredient = Ingredient { tag: None, items: ITEM_LIST_449 };
const INGREDIENT_455: Ingredient = Ingredient { tag: None, items: ITEM_LIST_450 };
const INGREDIENT_456: Ingredient = Ingredient { tag: None, items: ITEM_LIST_451 };
const INGREDIENT_457: Ingredient = Ingredient { tag: None, items: ITEM_LIST_452 };
const INGREDIENT_458: Ingredient = Ingredient { tag: None, items: ITEM_LIST_453 };
const INGREDIENT_459: Ingredient = Ingredient { tag: None, items: ITEM_LIST_454 };
const INGREDIENT_460: Ingredient = Ingredient { tag: None, items: ITEM_LIST_455 };
const INGREDIENT_461: Ingredient = Ingredient { tag: None, items: ITEM_LIST_456 };
const INGREDIENT_462: Ingredient = Ingredient { tag: None, items: ITEM_LIST_457 };
const INGREDIENT_463: Ingredient = Ingredient { tag: None, items: ITEM_LIST_458 };
const INGREDIENT_464: Ingredient = Ingredient { tag: None, items: ITEM_LIST_459 };
const INGREDIENT_465: Ingredient = Ingredient { tag: None, items: ITEM_LIST_460 };
const INGREDIENT_466: Ingredient = Ingredient { tag: None, items: ITEM_LIST_461 };
const INGREDIENT_467: Ingredient = Ingredient { tag: None, items: ITEM_LIST_462 };
const INGREDIENT_468: Ingredient = Ingredient { tag: None, items: ITEM_LIST_463 };
const INGREDIENT_469: Ingredient = Ingredient { tag: None, items: ITEM_LIST_464 };
const INGREDIENT_470: Ingredient = Ingredient { tag: None, items: ITEM_LIST_465 };
const INGREDIENT_471: Ingredient = Ingredient { tag: None, items: ITEM_LIST_466 };
const INGREDIENT_472: Ingredient = Ingredient { tag: None, items: ITEM_LIST_467 };
const INGREDIENT_473: Ingredient = Ingredient { tag: None, items: ITEM_LIST_468 };
const INGREDIENT_474: Ingredient = Ingredient { tag: None, items: ITEM_LIST_469 };
const INGREDIENT_475: Ingredient = Ingredient { tag: None, items: ITEM_LIST_470 };
const INGREDIENT_476: Ingredient = Ingredient { tag: Some("minecraft:mangrove_logs"), items: ITEM_LIST_471 };
const INGREDIENT_477: Ingredient = Ingredient { tag: None, items: ITEM_LIST_472 };
const INGREDIENT_478: Ingredient = Ingredient { tag: None, items: ITEM_LIST_473 };
const INGREDIENT_479: Ingredient = Ingredient { tag: Some("minecraft:clonable_maps"), items: ITEM_LIST_474 };
const INGREDIENT_480: Ingredient = Ingredient { tag: None, items: ITEM_LIST_475 };
const INGREDIENT_481: Ingredient = Ingredient { tag: Some("minecraft:extendable_maps"), items: ITEM_LIST_476 };
const INGREDIENT_482: Ingredient = Ingredient { tag: None, items: ITEM_LIST_477 };
const INGREDIENT_483: Ingredient = Ingredient { tag: None, items: ITEM_LIST_478 };
const INGREDIENT_484: Ingredient = Ingredient { tag: None, items: ITEM_LIST_479 };
const INGREDIENT_485: Ingredient = Ingredient { tag: None, items: ITEM_LIST_480 };
const INGREDIENT_486: Ingredient = Ingredient { tag: None, items: ITEM_LIST_481 };
const INGREDIENT_487: Ingredient = Ingredient { tag: None, items: ITEM_LIST_482 };
const INGREDIENT_488: Ingredient = Ingredient { tag: None, items: ITEM_LIST_483 };
const INGREDIENT_489: Ingredient = Ingredient { tag: None, items: ITEM_LIST_484 };
const INGREDIENT_490: Ingredient = Ingredient { tag: Some("minecraft:mushrooms"), items: ITEM_LIST_485 };
const INGREDIENT_491: Ingredient = Ingredient { tag: None, items: ITEM_LIST_486 };
const INGREDIENT_492: Ingredient = Ingredient { tag: Some("minecraft:metal_nuggets"), items: ITEM_LIST_487 };
const INGREDIENT_493: Ingredient = Ingredient { tag: None, items: ITEM_LIST_488 };
const INGREDIENT_494: Ingredient = Ingredient { tag: None, items: ITEM_LIST_489 };
const INGREDIENT_495: Ingredient = Ingredient { tag: None, items: ITEM_LIST_490 };
const INGREDIENT_496: Ingredient = Ingredient { tag: None, items: ITEM_LIST_491 };
const INGREDIENT_497: Ingredient = Ingredient { tag: Some("minecraft:netherite_tool_materials"), items: ITEM_LIST_492 };
const INGREDIENT_498: Ingredient = Ingredient { tag: None, items: ITEM_LIST_492 };
const INGREDIENT_499: Ingredient = Ingredient { tag: None, items: ITEM_LIST_493 };
const INGREDIENT_500: Ingredient = Ingredient { tag: None, items: ITEM_LIST_494 };
const INGREDIENT_501: Ingredient = Ingredient { tag: None, items: ITEM_LIST_495 };
const INGREDIENT_502: Ingredient = Ingredient { tag: None, items: ITEM_LIST_496 };
const INGREDIENT_503: Ingredient = Ingredient { tag: None, items: ITEM_LIST_497 };
const INGREDIENT_504: Ingredient = Ingredient { tag: None, items: ITEM_LIST_498 };
const INGREDIENT_505: Ingredient = Ingredient { tag: None, items: ITEM_LIST_499 };
const INGREDIENT_506: Ingredient = Ingredient { tag: None, items: ITEM_LIST_500 };
const INGREDIENT_507: Ingredient = Ingredient { tag: None, items: ITEM_LIST_501 };
const INGREDIENT_508: Ingredient = Ingredient { tag: None, items: ITEM_LIST_502 };
const INGREDIENT_509: Ingredient = Ingredient { tag: None, items: ITEM_LIST_503 };
const INGREDIENT_510: Ingredient = Ingredient { tag: None, items: ITEM_LIST_504 };
const INGREDIENT_511: Ingredient = Ingredient { tag: None, items: ITEM_LIST_505 };
const INGREDIENT_512: Ingredient = Ingredient { tag: None, items: ITEM_LIST_506 };
const INGREDIENT_513: Ingredient = Ingredient { tag: None, items: ITEM_LIST_507 };
const INGREDIENT_514: Ingredient = Ingredient { tag: None, items: ITEM_LIST_508 };
const INGREDIENT_515: Ingredient = Ingredient { tag: None, items: ITEM_LIST_509 };
const INGREDIENT_516: Ingredient = Ingredient { tag: Some("minecraft:oak_logs"), items: ITEM_LIST_510 };
const INGREDIENT_517: Ingredient = Ingredient { tag: None, items: ITEM_LIST_511 };
const INGREDIENT_518: Ingredient = Ingredient { tag: None, items: ITEM_LIST_512 };
const INGREDIENT_519: Ingredient = Ingredient { tag: None, items: ITEM_LIST_513 };
const INGREDIENT_520: Ingredient = Ingredient { tag: None, items: ITEM_LIST_514 };
const INGREDIENT_521: Ingredient = Ingredient { tag: None, items: ITEM_LIST_515 };
const INGREDIENT_522: Ingredient = Ingredient { tag: None, items: ITEM_LIST_516 };
const INGREDIENT_523: Ingredient = Ingredient { tag: None, items: ITEM_LIST_517 };
const INGREDIENT_524: Ingredient = Ingredient { tag: None, items: ITEM_LIST_518 };
const INGREDIENT_525: Ingredient = Ingredient { tag: None, items: ITEM_LIST_519 };
const INGREDIENT_526: Ingredient = Ingredient { tag: None, items: ITEM_LIST_520 };
const INGREDIENT_527: Ingredient = Ingredient { tag: None, items: ITEM_LIST_521 };
const INGREDIENT_528: Ingredient = Ingredient { tag: None, items: ITEM_LIST_522 };
const INGREDIENT_529: Ingredient = Ingredient { tag: None, items: ITEM_LIST_523 };
const INGREDIENT_530: Ingredient = Ingredient { tag: None, items: ITEM_LIST_524 };
const INGREDIENT_531: Ingredient = Ingredient { tag: Some("minecraft:wool"), items: ITEM_LIST_525 };
const INGREDIENT_532: Ingredient = Ingredient { tag: None, items: ITEM_LIST_526 };
const INGREDIENT_533: Ingredient = Ingredient { tag: None, items: ITEM_LIST_527 };
const INGREDIENT_534: Ingredient = Ingredient { tag: None, items: ITEM_LIST_528 };
const INGREDIENT_535: Ingredient = Ingredient { tag: None, items: ITEM_LIST_529 };
const INGREDIENT_536: Ingredient = Ingredient { tag: Some("minecraft:pale_oak_logs"), items: ITEM_LIST_530 };
const INGREDIENT_537: Ingredient = Ingredient { tag: None, items: ITEM_LIST_531 };
const INGREDIENT_538: Ingredient = Ingredient { tag: None, items: ITEM_LIST_532 };
const INGREDIENT_539: Ingredient = Ingredient { tag: None, items: ITEM_LIST_533 };
const INGREDIENT_540: Ingredient = Ingredient { tag: None, items: ITEM_LIST_534 };
const INGREDIENT_541: Ingredient = Ingredient { tag: None, items: ITEM_LIST_535 };
const INGREDIENT_542: Ingredient = Ingredient { tag: None, items: ITEM_LIST_536 };
const INGREDIENT_543: Ingredient = Ingredient { tag: None, items: ITEM_LIST_537 };
const INGREDIENT_544: Ingredient = Ingredient { tag: None, items: ITEM_LIST_538 };
const INGREDIENT_545: Ingredient = Ingredient { tag: None, items: ITEM_LIST_539 };
const INGREDIENT_546: Ingredient = Ingredient { tag: None, items: ITEM_LIST_540 };
const INGREDIENT_547: Ingredient = Ingredient { tag: None, items: ITEM_LIST_541 };
const INGREDIENT_548: Ingredient = Ingredient { tag: None, items: ITEM_LIST_542 };
const INGREDIENT_549: Ingredient = Ingredient { tag: None, items: ITEM_LIST_543 };
const INGREDIENT_550: Ingredient = Ingredient { tag: None, items: ITEM_LIST_544 };
const INGREDIENT_551: Ingredient = Ingredient { tag: None, items: ITEM_LIST_545 };
const INGREDIENT_552: Ingredient = Ingredient { tag: None, items: ITEM_LIST_546 };
const INGREDIENT_553: Ingredient = Ingredient { tag: None, items: ITEM_LIST_547 };
const INGREDIENT_554: Ingredient = Ingredient { tag: None, items: ITEM_LIST_548 };
const INGREDIENT_555: Ingredient = Ingredient { tag: None, items: ITEM_LIST_549 };
const INGREDIENT_556: Ingredient = Ingredient { tag: Some("minecraft:poplar_logs"), items: ITEM_LIST_550 };
const INGREDIENT_557: Ingredient = Ingredient { tag: None, items: ITEM_LIST_551 };
const INGREDIENT_558: Ingredient = Ingredient { tag: None, items: ITEM_LIST_552 };
const INGREDIENT_559: Ingredient = Ingredient { tag: None, items: ITEM_LIST_553 };
const INGREDIENT_560: Ingredient = Ingredient { tag: None, items: ITEM_LIST_554 };
const INGREDIENT_561: Ingredient = Ingredient { tag: None, items: ITEM_LIST_555 };
const INGREDIENT_562: Ingredient = Ingredient { tag: None, items: ITEM_LIST_556 };
const INGREDIENT_563: Ingredient = Ingredient { tag: None, items: ITEM_LIST_557 };
const INGREDIENT_564: Ingredient = Ingredient { tag: None, items: ITEM_LIST_558 };
const INGREDIENT_565: Ingredient = Ingredient { tag: None, items: ITEM_LIST_559 };
const INGREDIENT_566: Ingredient = Ingredient { tag: None, items: ITEM_LIST_560 };
const INGREDIENT_567: Ingredient = Ingredient { tag: None, items: ITEM_LIST_561 };
const INGREDIENT_568: Ingredient = Ingredient { tag: None, items: ITEM_LIST_562 };
const INGREDIENT_569: Ingredient = Ingredient { tag: None, items: ITEM_LIST_563 };
const INGREDIENT_570: Ingredient = Ingredient { tag: None, items: ITEM_LIST_564 };
const INGREDIENT_571: Ingredient = Ingredient { tag: None, items: ITEM_LIST_565 };
const INGREDIENT_572: Ingredient = Ingredient { tag: None, items: ITEM_LIST_566 };
const INGREDIENT_573: Ingredient = Ingredient { tag: None, items: ITEM_LIST_567 };
const INGREDIENT_574: Ingredient = Ingredient { tag: None, items: ITEM_LIST_568 };
const INGREDIENT_575: Ingredient = Ingredient { tag: None, items: ITEM_LIST_569 };
const INGREDIENT_576: Ingredient = Ingredient { tag: None, items: ITEM_LIST_570 };
const INGREDIENT_577: Ingredient = Ingredient { tag: None, items: ITEM_LIST_571 };
const INGREDIENT_578: Ingredient = Ingredient { tag: None, items: ITEM_LIST_572 };
const INGREDIENT_579: Ingredient = Ingredient { tag: None, items: ITEM_LIST_573 };
const INGREDIENT_580: Ingredient = Ingredient { tag: None, items: ITEM_LIST_574 };
const INGREDIENT_581: Ingredient = Ingredient { tag: None, items: ITEM_LIST_575 };
const INGREDIENT_582: Ingredient = Ingredient { tag: None, items: ITEM_LIST_576 };
const INGREDIENT_583: Ingredient = Ingredient { tag: None, items: ITEM_LIST_577 };
const INGREDIENT_584: Ingredient = Ingredient { tag: None, items: ITEM_LIST_578 };
const INGREDIENT_585: Ingredient = Ingredient { tag: None, items: ITEM_LIST_579 };
const INGREDIENT_586: Ingredient = Ingredient { tag: None, items: ITEM_LIST_580 };
const INGREDIENT_587: Ingredient = Ingredient { tag: None, items: ITEM_LIST_581 };
const INGREDIENT_588: Ingredient = Ingredient { tag: None, items: ITEM_LIST_582 };
const INGREDIENT_589: Ingredient = Ingredient { tag: None, items: ITEM_LIST_583 };
const INGREDIENT_590: Ingredient = Ingredient { tag: None, items: ITEM_LIST_584 };
const INGREDIENT_591: Ingredient = Ingredient { tag: None, items: ITEM_LIST_585 };
const INGREDIENT_592: Ingredient = Ingredient { tag: None, items: ITEM_LIST_586 };
const INGREDIENT_593: Ingredient = Ingredient { tag: None, items: ITEM_LIST_587 };
const INGREDIENT_594: Ingredient = Ingredient { tag: None, items: ITEM_LIST_588 };
const INGREDIENT_595: Ingredient = Ingredient { tag: None, items: ITEM_LIST_589 };
const INGREDIENT_596: Ingredient = Ingredient { tag: None, items: ITEM_LIST_590 };
const INGREDIENT_597: Ingredient = Ingredient { tag: None, items: ITEM_LIST_591 };
const INGREDIENT_598: Ingredient = Ingredient { tag: None, items: ITEM_LIST_592 };
const INGREDIENT_599: Ingredient = Ingredient { tag: None, items: ITEM_LIST_593 };
const INGREDIENT_600: Ingredient = Ingredient { tag: None, items: ITEM_LIST_594 };
const INGREDIENT_601: Ingredient = Ingredient { tag: None, items: ITEM_LIST_595 };
const INGREDIENT_602: Ingredient = Ingredient { tag: None, items: ITEM_LIST_596 };
const INGREDIENT_603: Ingredient = Ingredient { tag: None, items: ITEM_LIST_597 };
const INGREDIENT_604: Ingredient = Ingredient { tag: None, items: ITEM_LIST_598 };
const INGREDIENT_605: Ingredient = Ingredient { tag: None, items: ITEM_LIST_599 };
const INGREDIENT_606: Ingredient = Ingredient { tag: None, items: ITEM_LIST_600 };
const INGREDIENT_607: Ingredient = Ingredient { tag: None, items: ITEM_LIST_601 };
const INGREDIENT_608: Ingredient = Ingredient { tag: Some("minecraft:wooden_tool_materials"), items: ITEM_LIST_26 };
const INGREDIENT_609: Ingredient = Ingredient { tag: Some("minecraft:banners"), items: ITEM_LIST_602 };
const INGREDIENT_610: Ingredient = Ingredient { tag: None, items: ITEM_LIST_603 };
const INGREDIENT_611: Ingredient = Ingredient { tag: None, items: ITEM_LIST_604 };
const INGREDIENT_612: Ingredient = Ingredient { tag: None, items: ITEM_LIST_605 };
const INGREDIENT_613: Ingredient = Ingredient { tag: None, items: ITEM_LIST_606 };
const INGREDIENT_614: Ingredient = Ingredient { tag: None, items: ITEM_LIST_607 };
const INGREDIENT_615: Ingredient = Ingredient { tag: None, items: ITEM_LIST_608 };
const INGREDIENT_616: Ingredient = Ingredient { tag: None, items: ITEM_LIST_609 };
const INGREDIENT_617: Ingredient = Ingredient { tag: None, items: ITEM_LIST_610 };
const INGREDIENT_618: Ingredient = Ingredient { tag: None, items: ITEM_LIST_611 };
const INGREDIENT_619: Ingredient = Ingredient { tag: None, items: ITEM_LIST_612 };
const INGREDIENT_620: Ingredient = Ingredient { tag: Some("minecraft:soul_fire_base_blocks"), items: ITEM_LIST_613 };
const INGREDIENT_621: Ingredient = Ingredient { tag: None, items: ITEM_LIST_614 };
const INGREDIENT_622: Ingredient = Ingredient { tag: None, items: ITEM_LIST_615 };
const INGREDIENT_623: Ingredient = Ingredient { tag: None, items: ITEM_LIST_616 };
const INGREDIENT_624: Ingredient = Ingredient { tag: None, items: ITEM_LIST_617 };
const INGREDIENT_625: Ingredient = Ingredient { tag: None, items: ITEM_LIST_618 };
const INGREDIENT_626: Ingredient = Ingredient { tag: None, items: ITEM_LIST_619 };
const INGREDIENT_627: Ingredient = Ingredient { tag: None, items: ITEM_LIST_620 };
const INGREDIENT_628: Ingredient = Ingredient { tag: Some("minecraft:spruce_logs"), items: ITEM_LIST_621 };
const INGREDIENT_629: Ingredient = Ingredient { tag: None, items: ITEM_LIST_622 };
const INGREDIENT_630: Ingredient = Ingredient { tag: None, items: ITEM_LIST_623 };
const INGREDIENT_631: Ingredient = Ingredient { tag: Some("minecraft:stone_tool_materials"), items: ITEM_LIST_109 };
const INGREDIENT_632: Ingredient = Ingredient { tag: None, items: ITEM_LIST_624 };
const INGREDIENT_633: Ingredient = Ingredient { tag: None, items: ITEM_LIST_625 };
const INGREDIENT_634: Ingredient = Ingredient { tag: None, items: ITEM_LIST_626 };
const INGREDIENT_635: Ingredient = Ingredient { tag: None, items: ITEM_LIST_627 };
const INGREDIENT_636: Ingredient = Ingredient { tag: None, items: ITEM_LIST_628 };
const INGREDIENT_637: Ingredient = Ingredient { tag: None, items: ITEM_LIST_629 };
const INGREDIENT_638: Ingredient = Ingredient { tag: None, items: ITEM_LIST_630 };
const INGREDIENT_639: Ingredient = Ingredient { tag: None, items: ITEM_LIST_631 };
const INGREDIENT_640: Ingredient = Ingredient { tag: None, items: ITEM_LIST_379 };
const INGREDIENT_641: Ingredient = Ingredient { tag: None, items: ITEM_LIST_632 };
const INGREDIENT_642: Ingredient = Ingredient { tag: None, items: ITEM_LIST_633 };
const INGREDIENT_643: Ingredient = Ingredient { tag: None, items: ITEM_LIST_634 };
const INGREDIENT_644: Ingredient = Ingredient { tag: None, items: ITEM_LIST_635 };
const INGREDIENT_645: Ingredient = Ingredient { tag: None, items: ITEM_LIST_636 };
const INGREDIENT_646: Ingredient = Ingredient { tag: None, items: ITEM_LIST_637 };
const INGREDIENT_647: Ingredient = Ingredient { tag: None, items: ITEM_LIST_638 };
const INGREDIENT_648: Ingredient = Ingredient { tag: Some("minecraft:warped_stems"), items: ITEM_LIST_639 };
const INGREDIENT_649: Ingredient = Ingredient { tag: None, items: ITEM_LIST_640 };
const INGREDIENT_650: Ingredient = Ingredient { tag: None, items: ITEM_LIST_641 };
const INGREDIENT_651: Ingredient = Ingredient { tag: None, items: ITEM_LIST_642 };
const INGREDIENT_652: Ingredient = Ingredient { tag: None, items: ITEM_LIST_643 };
const INGREDIENT_653: Ingredient = Ingredient { tag: None, items: ITEM_LIST_644 };
const INGREDIENT_654: Ingredient = Ingredient { tag: None, items: ITEM_LIST_645 };
const INGREDIENT_655: Ingredient = Ingredient { tag: None, items: ITEM_LIST_646 };
const INGREDIENT_656: Ingredient = Ingredient { tag: None, items: ITEM_LIST_647 };
const INGREDIENT_657: Ingredient = Ingredient { tag: None, items: ITEM_LIST_648 };
const INGREDIENT_658: Ingredient = Ingredient { tag: None, items: ITEM_LIST_649 };
const INGREDIENT_659: Ingredient = Ingredient { tag: None, items: ITEM_LIST_650 };
const INGREDIENT_660: Ingredient = Ingredient { tag: None, items: ITEM_LIST_651 };
const INGREDIENT_661: Ingredient = Ingredient { tag: None, items: ITEM_LIST_652 };
const INGREDIENT_662: Ingredient = Ingredient { tag: None, items: ITEM_LIST_653 };
const INGREDIENT_663: Ingredient = Ingredient { tag: None, items: ITEM_LIST_654 };
const INGREDIENT_664: Ingredient = Ingredient { tag: None, items: ITEM_LIST_655 };
const INGREDIENT_665: Ingredient = Ingredient { tag: None, items: ITEM_LIST_656 };
const INGREDIENT_666: Ingredient = Ingredient { tag: None, items: ITEM_LIST_657 };
const INGREDIENT_667: Ingredient = Ingredient { tag: None, items: ITEM_LIST_658 };
const INGREDIENT_668: Ingredient = Ingredient { tag: None, items: ITEM_LIST_659 };
const INGREDIENT_669: Ingredient = Ingredient { tag: None, items: ITEM_LIST_660 };
const INGREDIENT_670: Ingredient = Ingredient { tag: None, items: ITEM_LIST_661 };
const INGREDIENT_671: Ingredient = Ingredient { tag: None, items: ITEM_LIST_662 };
const INGREDIENT_672: Ingredient = Ingredient { tag: None, items: ITEM_LIST_663 };
const INGREDIENT_673: Ingredient = Ingredient { tag: None, items: ITEM_LIST_664 };
const INGREDIENT_674: Ingredient = Ingredient { tag: None, items: ITEM_LIST_665 };
const INGREDIENT_675: Ingredient = Ingredient { tag: None, items: ITEM_LIST_666 };
const INGREDIENT_676: Ingredient = Ingredient { tag: None, items: ITEM_LIST_667 };
const INGREDIENT_677: Ingredient = Ingredient { tag: None, items: ITEM_LIST_668 };
const INGREDIENT_678: Ingredient = Ingredient { tag: None, items: ITEM_LIST_669 };
const INGREDIENT_679: Ingredient = Ingredient { tag: None, items: ITEM_LIST_670 };
const INGREDIENT_680: Ingredient = Ingredient { tag: None, items: ITEM_LIST_671 };
const INGREDIENT_681: Ingredient = Ingredient { tag: None, items: ITEM_LIST_672 };
const INGREDIENT_682: Ingredient = Ingredient { tag: None, items: ITEM_LIST_673 };
const INGREDIENT_683: Ingredient = Ingredient { tag: None, items: ITEM_LIST_674 };
const INGREDIENT_684: Ingredient = Ingredient { tag: None, items: ITEM_LIST_675 };
const INGREDIENT_685: Ingredient = Ingredient { tag: None, items: ITEM_LIST_676 };
const INGREDIENT_686: Ingredient = Ingredient { tag: None, items: ITEM_LIST_677 };
const INGREDIENT_687: Ingredient = Ingredient { tag: None, items: ITEM_LIST_678 };
const INGREDIENT_688: Ingredient = Ingredient { tag: None, items: ITEM_LIST_679 };
const INGREDIENT_689: Ingredient = Ingredient { tag: None, items: ITEM_LIST_680 };
const INGREDIENT_690: Ingredient = Ingredient { tag: None, items: ITEM_LIST_681 };
const INGREDIENT_691: Ingredient = Ingredient { tag: None, items: ITEM_LIST_682 };
const INGREDIENT_692: Ingredient = Ingredient { tag: None, items: ITEM_LIST_683 };
const INGREDIENT_693: Ingredient = Ingredient { tag: None, items: ITEM_LIST_684 };
const INGREDIENT_694: Ingredient = Ingredient { tag: None, items: ITEM_LIST_685 };
const INGREDIENT_695: Ingredient = Ingredient { tag: None, items: ITEM_LIST_686 };
const INGREDIENT_696: Ingredient = Ingredient { tag: None, items: ITEM_LIST_687 };
const INGREDIENT_697: Ingredient = Ingredient { tag: None, items: ITEM_LIST_688 };
const INGREDIENT_698: Ingredient = Ingredient { tag: None, items: ITEM_LIST_689 };
const INGREDIENT_699: Ingredient = Ingredient { tag: None, items: ITEM_LIST_690 };
const INGREDIENT_700: Ingredient = Ingredient { tag: None, items: ITEM_LIST_691 };
const INGREDIENT_701: Ingredient = Ingredient { tag: None, items: ITEM_LIST_692 };
const INGREDIENT_702: Ingredient = Ingredient { tag: None, items: ITEM_LIST_693 };
const INGREDIENT_703: Ingredient = Ingredient { tag: None, items: ITEM_LIST_694 };
const INGREDIENT_704: Ingredient = Ingredient { tag: None, items: ITEM_LIST_695 };
const INGREDIENT_705: Ingredient = Ingredient { tag: None, items: ITEM_LIST_696 };
const INGREDIENT_706: Ingredient = Ingredient { tag: None, items: ITEM_LIST_697 };
const INGREDIENT_707: Ingredient = Ingredient { tag: None, items: ITEM_LIST_698 };
const INGREDIENT_708: Ingredient = Ingredient { tag: None, items: ITEM_LIST_699 };
const INGREDIENT_709: Ingredient = Ingredient { tag: None, items: ITEM_LIST_700 };
const INGREDIENT_710: Ingredient = Ingredient { tag: None, items: ITEM_LIST_701 };
const INGREDIENT_711: Ingredient = Ingredient { tag: None, items: ITEM_LIST_702 };
const INGREDIENT_712: Ingredient = Ingredient { tag: None, items: ITEM_LIST_703 };
const INGREDIENT_713: Ingredient = Ingredient { tag: None, items: ITEM_LIST_704 };
const INGREDIENT_714: Ingredient = Ingredient { tag: None, items: ITEM_LIST_705 };
const INGREDIENT_715: Ingredient = Ingredient { tag: None, items: ITEM_LIST_706 };
const INGREDIENT_716: Ingredient = Ingredient { tag: None, items: ITEM_LIST_707 };
const INGREDIENT_717: Ingredient = Ingredient { tag: None, items: ITEM_LIST_708 };
const INGREDIENT_718: Ingredient = Ingredient { tag: None, items: ITEM_LIST_709 };
const INGREDIENT_719: Ingredient = Ingredient { tag: None, items: ITEM_LIST_710 };
const INGREDIENT_720: Ingredient = Ingredient { tag: None, items: ITEM_LIST_711 };
const INGREDIENT_721: Ingredient = Ingredient { tag: None, items: ITEM_LIST_712 };
const INGREDIENT_722: Ingredient = Ingredient { tag: None, items: ITEM_LIST_713 };
const INGREDIENT_723: Ingredient = Ingredient { tag: None, items: ITEM_LIST_714 };
const INGREDIENT_724: Ingredient = Ingredient { tag: None, items: ITEM_LIST_715 };
const INGREDIENT_725: Ingredient = Ingredient { tag: None, items: ITEM_LIST_716 };
const INGREDIENT_726: Ingredient = Ingredient { tag: None, items: ITEM_LIST_717 };
const INGREDIENT_727: Ingredient = Ingredient { tag: None, items: ITEM_LIST_718 };
const INGREDIENT_728: Ingredient = Ingredient { tag: None, items: ITEM_LIST_719 };
const PATTERN_0: &[Option<Ingredient>] = &[Some(INGREDIENT_0), None, Some(INGREDIENT_0), Some(INGREDIENT_0), Some(INGREDIENT_0), Some(INGREDIENT_0)];
const PATTERN_1: &[Option<Ingredient>] = &[Some(INGREDIENT_0), Some(INGREDIENT_0), Some(INGREDIENT_0), Some(INGREDIENT_0), Some(INGREDIENT_0), Some(INGREDIENT_0)];
const PATTERN_2: &[Option<Ingredient>] = &[Some(INGREDIENT_0), Some(INGREDIENT_3), Some(INGREDIENT_0), Some(INGREDIENT_0), Some(INGREDIENT_3), Some(INGREDIENT_0)];
const PATTERN_3: &[Option<Ingredient>] = &[Some(INGREDIENT_3), Some(INGREDIENT_0), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_0), Some(INGREDIENT_3)];
const PATTERN_4: &[Option<Ingredient>] = &[Some(INGREDIENT_4), None, Some(INGREDIENT_4), Some(INGREDIENT_5), Some(INGREDIENT_5), Some(INGREDIENT_5), Some(INGREDIENT_5), Some(INGREDIENT_5), Some(INGREDIENT_5)];
const PATTERN_5: &[Option<Ingredient>] = &[Some(INGREDIENT_0), Some(INGREDIENT_0)];
const PATTERN_6: &[Option<Ingredient>] = &[Some(INGREDIENT_5), Some(INGREDIENT_5), Some(INGREDIENT_5), None, None, None, Some(INGREDIENT_5), Some(INGREDIENT_5), Some(INGREDIENT_5)];
const PATTERN_7: &[Option<Ingredient>] = &[Some(INGREDIENT_0), Some(INGREDIENT_0), Some(INGREDIENT_0), Some(INGREDIENT_0), Some(INGREDIENT_0), Some(INGREDIENT_0), None, Some(INGREDIENT_3), None];
const PATTERN_8: &[Option<Ingredient>] = &[Some(INGREDIENT_0), Some(INGREDIENT_0), Some(INGREDIENT_0)];
const PATTERN_9: &[Option<Ingredient>] = &[Some(INGREDIENT_0), None, None, Some(INGREDIENT_0), Some(INGREDIENT_0), None, Some(INGREDIENT_0), Some(INGREDIENT_0), Some(INGREDIENT_0)];
const PATTERN_10: &[Option<Ingredient>] = &[Some(INGREDIENT_7), Some(INGREDIENT_7), Some(INGREDIENT_7), Some(INGREDIENT_7)];
const PATTERN_11: &[Option<Ingredient>] = &[Some(INGREDIENT_8), Some(INGREDIENT_3), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_9), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_3), Some(INGREDIENT_8)];
const PATTERN_12: &[Option<Ingredient>] = &[Some(INGREDIENT_10), Some(INGREDIENT_10), Some(INGREDIENT_10), Some(INGREDIENT_10)];
const PATTERN_13: &[Option<Ingredient>] = &[Some(INGREDIENT_13), Some(INGREDIENT_13), Some(INGREDIENT_13)];
const PATTERN_14: &[Option<Ingredient>] = &[Some(INGREDIENT_13), None, None, Some(INGREDIENT_13), Some(INGREDIENT_13), None, Some(INGREDIENT_13), Some(INGREDIENT_13), Some(INGREDIENT_13)];
const PATTERN_15: &[Option<Ingredient>] = &[Some(INGREDIENT_13), Some(INGREDIENT_13), Some(INGREDIENT_13), Some(INGREDIENT_13), Some(INGREDIENT_13), Some(INGREDIENT_13)];
const PATTERN_16: &[Option<Ingredient>] = &[Some(INGREDIENT_14), Some(INGREDIENT_14), Some(INGREDIENT_14), None, Some(INGREDIENT_8), None, Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8)];
const PATTERN_17: &[Option<Ingredient>] = &[Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_3), None, Some(INGREDIENT_3), None, Some(INGREDIENT_3), Some(INGREDIENT_15), Some(INGREDIENT_3)];
const PATTERN_18: &[Option<Ingredient>] = &[Some(INGREDIENT_16), Some(INGREDIENT_3), Some(INGREDIENT_17)];
const PATTERN_19: &[Option<Ingredient>] = &[Some(INGREDIENT_20), Some(INGREDIENT_20), Some(INGREDIENT_20), Some(INGREDIENT_20), Some(INGREDIENT_20), Some(INGREDIENT_20)];
const PATTERN_20: &[Option<Ingredient>] = &[Some(INGREDIENT_20), Some(INGREDIENT_3), Some(INGREDIENT_20), Some(INGREDIENT_20), Some(INGREDIENT_3), Some(INGREDIENT_20)];
const PATTERN_21: &[Option<Ingredient>] = &[Some(INGREDIENT_3), Some(INGREDIENT_20), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_20), Some(INGREDIENT_3)];
const PATTERN_22: &[Option<Ingredient>] = &[Some(INGREDIENT_4), None, Some(INGREDIENT_4), Some(INGREDIENT_22), Some(INGREDIENT_22), Some(INGREDIENT_22), Some(INGREDIENT_22), Some(INGREDIENT_22), Some(INGREDIENT_22)];
const PATTERN_23: &[Option<Ingredient>] = &[Some(INGREDIENT_23), Some(INGREDIENT_23)];
const PATTERN_24: &[Option<Ingredient>] = &[Some(INGREDIENT_24), Some(INGREDIENT_24), Some(INGREDIENT_24)];
const PATTERN_25: &[Option<Ingredient>] = &[Some(INGREDIENT_24), None, None, Some(INGREDIENT_24), Some(INGREDIENT_24), None, Some(INGREDIENT_24), Some(INGREDIENT_24), Some(INGREDIENT_24)];
const PATTERN_26: &[Option<Ingredient>] = &[Some(INGREDIENT_20), Some(INGREDIENT_20)];
const PATTERN_27: &[Option<Ingredient>] = &[Some(INGREDIENT_20), None, Some(INGREDIENT_20), Some(INGREDIENT_20), Some(INGREDIENT_20), Some(INGREDIENT_20)];
const PATTERN_28: &[Option<Ingredient>] = &[Some(INGREDIENT_22), Some(INGREDIENT_22), Some(INGREDIENT_22), None, None, None, Some(INGREDIENT_22), Some(INGREDIENT_22), Some(INGREDIENT_22)];
const PATTERN_29: &[Option<Ingredient>] = &[Some(INGREDIENT_20), Some(INGREDIENT_20), Some(INGREDIENT_20), Some(INGREDIENT_20), Some(INGREDIENT_20), Some(INGREDIENT_20), None, Some(INGREDIENT_3), None];
const PATTERN_30: &[Option<Ingredient>] = &[Some(INGREDIENT_20), Some(INGREDIENT_20), Some(INGREDIENT_20)];
const PATTERN_31: &[Option<Ingredient>] = &[Some(INGREDIENT_20), None, None, Some(INGREDIENT_20), Some(INGREDIENT_20), None, Some(INGREDIENT_20), Some(INGREDIENT_20), Some(INGREDIENT_20)];
const PATTERN_32: &[Option<Ingredient>] = &[Some(INGREDIENT_26), Some(INGREDIENT_27), Some(INGREDIENT_26), Some(INGREDIENT_26), None, Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_27), Some(INGREDIENT_26)];
const PATTERN_33: &[Option<Ingredient>] = &[Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_29), Some(INGREDIENT_28), Some(INGREDIENT_30), Some(INGREDIENT_30), Some(INGREDIENT_30)];
const PATTERN_34: &[Option<Ingredient>] = &[Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_31), Some(INGREDIENT_31), Some(INGREDIENT_31), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_35: &[Option<Ingredient>] = &[Some(INGREDIENT_34), None, Some(INGREDIENT_34), Some(INGREDIENT_34), Some(INGREDIENT_34), Some(INGREDIENT_34)];
const PATTERN_36: &[Option<Ingredient>] = &[Some(INGREDIENT_34), Some(INGREDIENT_34), Some(INGREDIENT_34), Some(INGREDIENT_34), Some(INGREDIENT_34), Some(INGREDIENT_34)];
const PATTERN_37: &[Option<Ingredient>] = &[Some(INGREDIENT_34), Some(INGREDIENT_3), Some(INGREDIENT_34), Some(INGREDIENT_34), Some(INGREDIENT_3), Some(INGREDIENT_34)];
const PATTERN_38: &[Option<Ingredient>] = &[Some(INGREDIENT_3), Some(INGREDIENT_34), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_34), Some(INGREDIENT_3)];
const PATTERN_39: &[Option<Ingredient>] = &[Some(INGREDIENT_4), None, Some(INGREDIENT_4), Some(INGREDIENT_36), Some(INGREDIENT_36), Some(INGREDIENT_36), Some(INGREDIENT_36), Some(INGREDIENT_36), Some(INGREDIENT_36)];
const PATTERN_40: &[Option<Ingredient>] = &[Some(INGREDIENT_34), Some(INGREDIENT_34)];
const PATTERN_41: &[Option<Ingredient>] = &[Some(INGREDIENT_36), Some(INGREDIENT_36), Some(INGREDIENT_36), None, None, None, Some(INGREDIENT_36), Some(INGREDIENT_36), Some(INGREDIENT_36)];
const PATTERN_42: &[Option<Ingredient>] = &[Some(INGREDIENT_34), Some(INGREDIENT_34), Some(INGREDIENT_34), Some(INGREDIENT_34), Some(INGREDIENT_34), Some(INGREDIENT_34), None, Some(INGREDIENT_3), None];
const PATTERN_43: &[Option<Ingredient>] = &[Some(INGREDIENT_34), Some(INGREDIENT_34), Some(INGREDIENT_34)];
const PATTERN_44: &[Option<Ingredient>] = &[Some(INGREDIENT_34), None, None, Some(INGREDIENT_34), Some(INGREDIENT_34), None, Some(INGREDIENT_34), Some(INGREDIENT_34), Some(INGREDIENT_34)];
const PATTERN_45: &[Option<Ingredient>] = &[Some(INGREDIENT_38), Some(INGREDIENT_38), Some(INGREDIENT_38), Some(INGREDIENT_38)];
const PATTERN_46: &[Option<Ingredient>] = &[Some(INGREDIENT_39), Some(INGREDIENT_39), Some(INGREDIENT_39), Some(INGREDIENT_39), Some(INGREDIENT_39), Some(INGREDIENT_39), None, Some(INGREDIENT_3), None];
const PATTERN_47: &[Option<Ingredient>] = &[Some(INGREDIENT_39), Some(INGREDIENT_39), Some(INGREDIENT_39), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_48: &[Option<Ingredient>] = &[Some(INGREDIENT_39), Some(INGREDIENT_39)];
const PATTERN_49: &[Option<Ingredient>] = &[Some(INGREDIENT_46), Some(INGREDIENT_46), Some(INGREDIENT_46)];
const PATTERN_50: &[Option<Ingredient>] = &[Some(INGREDIENT_46), None, None, Some(INGREDIENT_46), Some(INGREDIENT_46), None, Some(INGREDIENT_46), Some(INGREDIENT_46), Some(INGREDIENT_46)];
const PATTERN_51: &[Option<Ingredient>] = &[Some(INGREDIENT_47), Some(INGREDIENT_47), Some(INGREDIENT_47)];
const PATTERN_52: &[Option<Ingredient>] = &[Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_28), Some(INGREDIENT_39), Some(INGREDIENT_28)];
const PATTERN_53: &[Option<Ingredient>] = &[Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_42), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28)];
const PATTERN_54: &[Option<Ingredient>] = &[Some(INGREDIENT_53), Some(INGREDIENT_53), Some(INGREDIENT_53), Some(INGREDIENT_53), Some(INGREDIENT_53), Some(INGREDIENT_53)];
const PATTERN_55: &[Option<Ingredient>] = &[Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_42), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54)];
const PATTERN_56: &[Option<Ingredient>] = &[Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_42), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55)];
const PATTERN_57: &[Option<Ingredient>] = &[Some(INGREDIENT_39), Some(INGREDIENT_39), Some(INGREDIENT_39)];
const PATTERN_58: &[Option<Ingredient>] = &[Some(INGREDIENT_39), None, None, Some(INGREDIENT_39), Some(INGREDIENT_39), None, Some(INGREDIENT_39), Some(INGREDIENT_39), Some(INGREDIENT_39)];
const PATTERN_59: &[Option<Ingredient>] = &[Some(INGREDIENT_56), Some(INGREDIENT_56), Some(INGREDIENT_56)];
const PATTERN_60: &[Option<Ingredient>] = &[Some(INGREDIENT_56), None, None, Some(INGREDIENT_56), Some(INGREDIENT_56), None, Some(INGREDIENT_56), Some(INGREDIENT_56), Some(INGREDIENT_56)];
const PATTERN_61: &[Option<Ingredient>] = &[Some(INGREDIENT_56), Some(INGREDIENT_56), Some(INGREDIENT_56), Some(INGREDIENT_56), Some(INGREDIENT_56), Some(INGREDIENT_56)];
const PATTERN_62: &[Option<Ingredient>] = &[Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_57), Some(INGREDIENT_8), Some(INGREDIENT_58), Some(INGREDIENT_58), Some(INGREDIENT_58)];
const PATTERN_63: &[Option<Ingredient>] = &[Some(INGREDIENT_60), Some(INGREDIENT_60), Some(INGREDIENT_60), Some(INGREDIENT_60), Some(INGREDIENT_60), Some(INGREDIENT_60), None, Some(INGREDIENT_3), None];
const PATTERN_64: &[Option<Ingredient>] = &[Some(INGREDIENT_60), Some(INGREDIENT_60), Some(INGREDIENT_60), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_65: &[Option<Ingredient>] = &[Some(INGREDIENT_60), Some(INGREDIENT_60)];
const PATTERN_66: &[Option<Ingredient>] = &[Some(INGREDIENT_63), Some(INGREDIENT_63), Some(INGREDIENT_63)];
const PATTERN_67: &[Option<Ingredient>] = &[Some(INGREDIENT_63), None, None, Some(INGREDIENT_63), Some(INGREDIENT_63), None, Some(INGREDIENT_63), Some(INGREDIENT_63), Some(INGREDIENT_63)];
const PATTERN_68: &[Option<Ingredient>] = &[Some(INGREDIENT_64), Some(INGREDIENT_64), Some(INGREDIENT_64)];
const PATTERN_69: &[Option<Ingredient>] = &[Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_28), Some(INGREDIENT_60), Some(INGREDIENT_28)];
const PATTERN_70: &[Option<Ingredient>] = &[Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_62), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28)];
const PATTERN_71: &[Option<Ingredient>] = &[Some(INGREDIENT_69), Some(INGREDIENT_69), Some(INGREDIENT_69), Some(INGREDIENT_69), Some(INGREDIENT_69), Some(INGREDIENT_69)];
const PATTERN_72: &[Option<Ingredient>] = &[Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_62), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54)];
const PATTERN_73: &[Option<Ingredient>] = &[Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_62), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55)];
const PATTERN_74: &[Option<Ingredient>] = &[Some(INGREDIENT_60), Some(INGREDIENT_60), Some(INGREDIENT_60)];
const PATTERN_75: &[Option<Ingredient>] = &[Some(INGREDIENT_60), None, None, Some(INGREDIENT_60), Some(INGREDIENT_60), None, Some(INGREDIENT_60), Some(INGREDIENT_60), Some(INGREDIENT_60)];
const PATTERN_76: &[Option<Ingredient>] = &[Some(INGREDIENT_70), Some(INGREDIENT_71), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_72), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70)];
const PATTERN_77: &[Option<Ingredient>] = &[Some(INGREDIENT_75), Some(INGREDIENT_75), Some(INGREDIENT_75), Some(INGREDIENT_75), Some(INGREDIENT_75), Some(INGREDIENT_75), Some(INGREDIENT_75), Some(INGREDIENT_75), Some(INGREDIENT_75)];
const PATTERN_78: &[Option<Ingredient>] = &[Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_81), Some(INGREDIENT_81), Some(INGREDIENT_81), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_79: &[Option<Ingredient>] = &[None, Some(INGREDIENT_3), Some(INGREDIENT_83), Some(INGREDIENT_3), None, Some(INGREDIENT_83), None, Some(INGREDIENT_3), Some(INGREDIENT_83)];
const PATTERN_80: &[Option<Ingredient>] = &[Some(INGREDIENT_26), None, Some(INGREDIENT_26), None, Some(INGREDIENT_26), None];
const PATTERN_81: &[Option<Ingredient>] = &[Some(INGREDIENT_84), Some(INGREDIENT_84), Some(INGREDIENT_84)];
const PATTERN_82: &[Option<Ingredient>] = &[None, Some(INGREDIENT_59), None, Some(INGREDIENT_109), Some(INGREDIENT_109), Some(INGREDIENT_109)];
const PATTERN_83: &[Option<Ingredient>] = &[Some(INGREDIENT_111), Some(INGREDIENT_111), Some(INGREDIENT_111)];
const PATTERN_84: &[Option<Ingredient>] = &[Some(INGREDIENT_111), None, None, Some(INGREDIENT_111), Some(INGREDIENT_111), None, Some(INGREDIENT_111), Some(INGREDIENT_111), Some(INGREDIENT_111)];
const PATTERN_85: &[Option<Ingredient>] = &[Some(INGREDIENT_111), Some(INGREDIENT_111), Some(INGREDIENT_111), Some(INGREDIENT_111), Some(INGREDIENT_111), Some(INGREDIENT_111)];
const PATTERN_86: &[Option<Ingredient>] = &[Some(INGREDIENT_112), Some(INGREDIENT_112), Some(INGREDIENT_112), Some(INGREDIENT_112)];
const PATTERN_87: &[Option<Ingredient>] = &[Some(INGREDIENT_113), Some(INGREDIENT_113), Some(INGREDIENT_113), Some(INGREDIENT_113), Some(INGREDIENT_113), Some(INGREDIENT_113), None, Some(INGREDIENT_3), None];
const PATTERN_88: &[Option<Ingredient>] = &[Some(INGREDIENT_113), Some(INGREDIENT_113), Some(INGREDIENT_113), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_89: &[Option<Ingredient>] = &[Some(INGREDIENT_113), Some(INGREDIENT_113)];
const PATTERN_90: &[Option<Ingredient>] = &[Some(INGREDIENT_116), Some(INGREDIENT_116), Some(INGREDIENT_116)];
const PATTERN_91: &[Option<Ingredient>] = &[Some(INGREDIENT_116), None, None, Some(INGREDIENT_116), Some(INGREDIENT_116), None, Some(INGREDIENT_116), Some(INGREDIENT_116), Some(INGREDIENT_116)];
const PATTERN_92: &[Option<Ingredient>] = &[Some(INGREDIENT_117), Some(INGREDIENT_117), Some(INGREDIENT_117)];
const PATTERN_93: &[Option<Ingredient>] = &[Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_28), Some(INGREDIENT_113), Some(INGREDIENT_28)];
const PATTERN_94: &[Option<Ingredient>] = &[Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_115), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28)];
const PATTERN_95: &[Option<Ingredient>] = &[Some(INGREDIENT_120), Some(INGREDIENT_120), Some(INGREDIENT_120), Some(INGREDIENT_120), Some(INGREDIENT_120), Some(INGREDIENT_120)];
const PATTERN_96: &[Option<Ingredient>] = &[Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_115), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54)];
const PATTERN_97: &[Option<Ingredient>] = &[Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_115), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55)];
const PATTERN_98: &[Option<Ingredient>] = &[Some(INGREDIENT_113), Some(INGREDIENT_113), Some(INGREDIENT_113)];
const PATTERN_99: &[Option<Ingredient>] = &[Some(INGREDIENT_113), None, None, Some(INGREDIENT_113), Some(INGREDIENT_113), None, Some(INGREDIENT_113), Some(INGREDIENT_113), Some(INGREDIENT_113)];
const PATTERN_100: &[Option<Ingredient>] = &[Some(INGREDIENT_17), Some(INGREDIENT_121), Some(INGREDIENT_3)];
const PATTERN_101: &[Option<Ingredient>] = &[Some(INGREDIENT_8), None, Some(INGREDIENT_8), None, Some(INGREDIENT_8), None];
const PATTERN_102: &[Option<Ingredient>] = &[Some(INGREDIENT_83), Some(INGREDIENT_51)];
const PATTERN_103: &[Option<Ingredient>] = &[Some(INGREDIENT_122), Some(INGREDIENT_122), Some(INGREDIENT_122), Some(INGREDIENT_99), Some(INGREDIENT_123), Some(INGREDIENT_99), Some(INGREDIENT_84), Some(INGREDIENT_84), Some(INGREDIENT_84)];
const PATTERN_104: &[Option<Ingredient>] = &[None, Some(INGREDIENT_10), None, Some(INGREDIENT_10), Some(INGREDIENT_124), Some(INGREDIENT_10)];
const PATTERN_105: &[Option<Ingredient>] = &[None, Some(INGREDIENT_3), None, Some(INGREDIENT_3), Some(INGREDIENT_125), Some(INGREDIENT_3), Some(INGREDIENT_126), Some(INGREDIENT_126), Some(INGREDIENT_126)];
const PATTERN_106: &[Option<Ingredient>] = &[Some(INGREDIENT_83), Some(INGREDIENT_31)];
const PATTERN_107: &[Option<Ingredient>] = &[Some(INGREDIENT_127), None, None, Some(INGREDIENT_128)];
const PATTERN_108: &[Option<Ingredient>] = &[Some(INGREDIENT_78), Some(INGREDIENT_78), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_109: &[Option<Ingredient>] = &[Some(INGREDIENT_8), None, Some(INGREDIENT_8), Some(INGREDIENT_8), None, Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8)];
const PATTERN_110: &[Option<Ingredient>] = &[Some(INGREDIENT_130), None, Some(INGREDIENT_130), Some(INGREDIENT_130), Some(INGREDIENT_130), Some(INGREDIENT_130)];
const PATTERN_111: &[Option<Ingredient>] = &[Some(INGREDIENT_130), Some(INGREDIENT_130), Some(INGREDIENT_130), Some(INGREDIENT_130), Some(INGREDIENT_130), Some(INGREDIENT_130)];
const PATTERN_112: &[Option<Ingredient>] = &[Some(INGREDIENT_130), Some(INGREDIENT_3), Some(INGREDIENT_130), Some(INGREDIENT_130), Some(INGREDIENT_3), Some(INGREDIENT_130)];
const PATTERN_113: &[Option<Ingredient>] = &[Some(INGREDIENT_3), Some(INGREDIENT_130), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_130), Some(INGREDIENT_3)];
const PATTERN_114: &[Option<Ingredient>] = &[Some(INGREDIENT_4), None, Some(INGREDIENT_4), Some(INGREDIENT_132), Some(INGREDIENT_132), Some(INGREDIENT_132), Some(INGREDIENT_132), Some(INGREDIENT_132), Some(INGREDIENT_132)];
const PATTERN_115: &[Option<Ingredient>] = &[Some(INGREDIENT_130), Some(INGREDIENT_130)];
const PATTERN_116: &[Option<Ingredient>] = &[Some(INGREDIENT_132), Some(INGREDIENT_132), Some(INGREDIENT_132), None, None, None, Some(INGREDIENT_132), Some(INGREDIENT_132), Some(INGREDIENT_132)];
const PATTERN_117: &[Option<Ingredient>] = &[Some(INGREDIENT_130), Some(INGREDIENT_130), Some(INGREDIENT_130), Some(INGREDIENT_130), Some(INGREDIENT_130), Some(INGREDIENT_130), None, Some(INGREDIENT_3), None];
const PATTERN_118: &[Option<Ingredient>] = &[Some(INGREDIENT_130), Some(INGREDIENT_130), Some(INGREDIENT_130)];
const PATTERN_119: &[Option<Ingredient>] = &[Some(INGREDIENT_130), None, None, Some(INGREDIENT_130), Some(INGREDIENT_130), None, Some(INGREDIENT_130), Some(INGREDIENT_130), Some(INGREDIENT_130)];
const PATTERN_120: &[Option<Ingredient>] = &[Some(INGREDIENT_134), Some(INGREDIENT_134), Some(INGREDIENT_134), Some(INGREDIENT_134)];
const PATTERN_121: &[Option<Ingredient>] = &[Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26), None, Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_122: &[Option<Ingredient>] = &[Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_27), Some(INGREDIENT_27), Some(INGREDIENT_27), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_123: &[Option<Ingredient>] = &[Some(INGREDIENT_136), Some(INGREDIENT_136)];
const PATTERN_124: &[Option<Ingredient>] = &[Some(INGREDIENT_138), Some(INGREDIENT_138)];
const PATTERN_125: &[Option<Ingredient>] = &[Some(INGREDIENT_141), Some(INGREDIENT_141)];
const PATTERN_126: &[Option<Ingredient>] = &[Some(INGREDIENT_144), Some(INGREDIENT_144)];
const PATTERN_127: &[Option<Ingredient>] = &[Some(INGREDIENT_146), Some(INGREDIENT_146)];
const PATTERN_128: &[Option<Ingredient>] = &[Some(INGREDIENT_148), Some(INGREDIENT_148)];
const PATTERN_129: &[Option<Ingredient>] = &[Some(INGREDIENT_150), Some(INGREDIENT_150)];
const PATTERN_130: &[Option<Ingredient>] = &[Some(INGREDIENT_152), Some(INGREDIENT_152)];
const PATTERN_131: &[Option<Ingredient>] = &[Some(INGREDIENT_154), Some(INGREDIENT_154)];
const PATTERN_132: &[Option<Ingredient>] = &[Some(INGREDIENT_156), Some(INGREDIENT_156)];
const PATTERN_133: &[Option<Ingredient>] = &[Some(INGREDIENT_158), Some(INGREDIENT_158)];
const PATTERN_134: &[Option<Ingredient>] = &[Some(INGREDIENT_160), Some(INGREDIENT_160)];
const PATTERN_135: &[Option<Ingredient>] = &[Some(INGREDIENT_161), Some(INGREDIENT_161)];
const PATTERN_136: &[Option<Ingredient>] = &[Some(INGREDIENT_165), Some(INGREDIENT_165), Some(INGREDIENT_165)];
const PATTERN_137: &[Option<Ingredient>] = &[Some(INGREDIENT_165), None, None, Some(INGREDIENT_165), Some(INGREDIENT_165), None, Some(INGREDIENT_165), Some(INGREDIENT_165), Some(INGREDIENT_165)];
const PATTERN_138: &[Option<Ingredient>] = &[Some(INGREDIENT_165), Some(INGREDIENT_165), Some(INGREDIENT_165), Some(INGREDIENT_165), Some(INGREDIENT_165), Some(INGREDIENT_165)];
const PATTERN_139: &[Option<Ingredient>] = &[Some(INGREDIENT_166), Some(INGREDIENT_166), Some(INGREDIENT_166), Some(INGREDIENT_166)];
const PATTERN_140: &[Option<Ingredient>] = &[Some(INGREDIENT_137), Some(INGREDIENT_137), Some(INGREDIENT_137)];
const PATTERN_141: &[Option<Ingredient>] = &[Some(INGREDIENT_137), None, None, Some(INGREDIENT_137), Some(INGREDIENT_137), None, Some(INGREDIENT_137), Some(INGREDIENT_137), Some(INGREDIENT_137)];
const PATTERN_142: &[Option<Ingredient>] = &[Some(INGREDIENT_137), Some(INGREDIENT_137), Some(INGREDIENT_137), Some(INGREDIENT_137), Some(INGREDIENT_137), Some(INGREDIENT_137)];
const PATTERN_143: &[Option<Ingredient>] = &[Some(INGREDIENT_110), Some(INGREDIENT_110), Some(INGREDIENT_110), Some(INGREDIENT_110)];
const PATTERN_144: &[Option<Ingredient>] = &[None, Some(INGREDIENT_167), None, Some(INGREDIENT_167), Some(INGREDIENT_101), Some(INGREDIENT_167), None, Some(INGREDIENT_167), None];
const PATTERN_145: &[Option<Ingredient>] = &[Some(INGREDIENT_169), Some(INGREDIENT_169), Some(INGREDIENT_169), Some(INGREDIENT_169), Some(INGREDIENT_169), Some(INGREDIENT_169), Some(INGREDIENT_169), Some(INGREDIENT_169), Some(INGREDIENT_169)];
const PATTERN_146: &[Option<Ingredient>] = &[Some(INGREDIENT_172), Some(INGREDIENT_45), Some(INGREDIENT_45), Some(INGREDIENT_172)];
const PATTERN_147: &[Option<Ingredient>] = &[Some(INGREDIENT_70), Some(INGREDIENT_173), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_12), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70)];
const PATTERN_148: &[Option<Ingredient>] = &[Some(INGREDIENT_142), Some(INGREDIENT_142), Some(INGREDIENT_142)];
const PATTERN_149: &[Option<Ingredient>] = &[Some(INGREDIENT_142), None, None, Some(INGREDIENT_142), Some(INGREDIENT_142), None, Some(INGREDIENT_142), Some(INGREDIENT_142), Some(INGREDIENT_142)];
const PATTERN_150: &[Option<Ingredient>] = &[Some(INGREDIENT_142), Some(INGREDIENT_142), Some(INGREDIENT_142), Some(INGREDIENT_142), Some(INGREDIENT_142), Some(INGREDIENT_142)];
const PATTERN_151: &[Option<Ingredient>] = &[Some(INGREDIENT_12), Some(INGREDIENT_12), Some(INGREDIENT_12)];
const PATTERN_152: &[Option<Ingredient>] = &[Some(INGREDIENT_12), None, None, Some(INGREDIENT_12), Some(INGREDIENT_12), None, Some(INGREDIENT_12), Some(INGREDIENT_12), Some(INGREDIENT_12)];
const PATTERN_153: &[Option<Ingredient>] = &[Some(INGREDIENT_12), Some(INGREDIENT_12), Some(INGREDIENT_12), Some(INGREDIENT_12), Some(INGREDIENT_12), Some(INGREDIENT_12)];
const PATTERN_154: &[Option<Ingredient>] = &[None, Some(INGREDIENT_9), None, Some(INGREDIENT_9), Some(INGREDIENT_174), Some(INGREDIENT_9), Some(INGREDIENT_98), Some(INGREDIENT_98), Some(INGREDIENT_98)];
const PATTERN_155: &[Option<Ingredient>] = &[None, Some(INGREDIENT_8), None, Some(INGREDIENT_8), Some(INGREDIENT_101), Some(INGREDIENT_8), None, Some(INGREDIENT_8), None];
const PATTERN_156: &[Option<Ingredient>] = &[Some(INGREDIENT_27), None, Some(INGREDIENT_27), Some(INGREDIENT_27), None, Some(INGREDIENT_27), Some(INGREDIENT_27), Some(INGREDIENT_27), Some(INGREDIENT_27)];
const PATTERN_157: &[Option<Ingredient>] = &[Some(INGREDIENT_175), Some(INGREDIENT_175), Some(INGREDIENT_175), Some(INGREDIENT_175), Some(INGREDIENT_176), Some(INGREDIENT_175), Some(INGREDIENT_175), Some(INGREDIENT_175), Some(INGREDIENT_175)];
const PATTERN_158: &[Option<Ingredient>] = &[Some(INGREDIENT_84), Some(INGREDIENT_118), Some(INGREDIENT_84)];
const PATTERN_159: &[Option<Ingredient>] = &[Some(INGREDIENT_184), Some(INGREDIENT_184), Some(INGREDIENT_184), Some(INGREDIENT_3), None, Some(INGREDIENT_3)];
const PATTERN_160: &[Option<Ingredient>] = &[Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121)];
const PATTERN_161: &[Option<Ingredient>] = &[Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121)];
const PATTERN_162: &[Option<Ingredient>] = &[Some(INGREDIENT_121), None, Some(INGREDIENT_121), Some(INGREDIENT_121), None, Some(INGREDIENT_121)];
const PATTERN_163: &[Option<Ingredient>] = &[None, Some(INGREDIENT_139), None, Some(INGREDIENT_139), Some(INGREDIENT_59), Some(INGREDIENT_139), None, Some(INGREDIENT_101), None];
const PATTERN_164: &[Option<Ingredient>] = &[Some(INGREDIENT_185), Some(INGREDIENT_121), Some(INGREDIENT_185)];
const PATTERN_165: &[Option<Ingredient>] = &[Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_1), Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121)];
const PATTERN_166: &[Option<Ingredient>] = &[Some(INGREDIENT_121), None, Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121)];
const PATTERN_167: &[Option<Ingredient>] = &[None, Some(INGREDIENT_139), None, Some(INGREDIENT_139), None, Some(INGREDIENT_139), None, Some(INGREDIENT_139), None];
const PATTERN_168: &[Option<Ingredient>] = &[Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121), None, Some(INGREDIENT_121)];
const PATTERN_169: &[Option<Ingredient>] = &[Some(INGREDIENT_184), Some(INGREDIENT_184), None, Some(INGREDIENT_3), None, Some(INGREDIENT_3)];
const PATTERN_170: &[Option<Ingredient>] = &[Some(INGREDIENT_185), Some(INGREDIENT_185), Some(INGREDIENT_185), Some(INGREDIENT_185), Some(INGREDIENT_185), Some(INGREDIENT_185), Some(INGREDIENT_185), Some(INGREDIENT_185), Some(INGREDIENT_185)];
const PATTERN_171: &[Option<Ingredient>] = &[Some(INGREDIENT_185), Some(INGREDIENT_185), Some(INGREDIENT_185), Some(INGREDIENT_185), Some(INGREDIENT_190), Some(INGREDIENT_185), Some(INGREDIENT_185), Some(INGREDIENT_185), Some(INGREDIENT_185)];
const PATTERN_172: &[Option<Ingredient>] = &[Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121), None, Some(INGREDIENT_121), Some(INGREDIENT_121), None, Some(INGREDIENT_121)];
const PATTERN_173: &[Option<Ingredient>] = &[Some(INGREDIENT_184), Some(INGREDIENT_184), Some(INGREDIENT_184), None, Some(INGREDIENT_3), None, None, Some(INGREDIENT_3), None];
const PATTERN_174: &[Option<Ingredient>] = &[Some(INGREDIENT_184), Some(INGREDIENT_3), Some(INGREDIENT_3)];
const PATTERN_175: &[Option<Ingredient>] = &[None, None, Some(INGREDIENT_184), None, Some(INGREDIENT_3), None, Some(INGREDIENT_3), None, None];
const PATTERN_176: &[Option<Ingredient>] = &[Some(INGREDIENT_184), Some(INGREDIENT_184), Some(INGREDIENT_3)];
const PATTERN_177: &[Option<Ingredient>] = &[Some(INGREDIENT_185), Some(INGREDIENT_192), Some(INGREDIENT_3)];
const PATTERN_178: &[Option<Ingredient>] = &[Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121)];
const PATTERN_179: &[Option<Ingredient>] = &[Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_196), Some(INGREDIENT_8), Some(INGREDIENT_101), Some(INGREDIENT_197), Some(INGREDIENT_101)];
const PATTERN_180: &[Option<Ingredient>] = &[Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_181: &[Option<Ingredient>] = &[Some(INGREDIENT_198), Some(INGREDIENT_199), Some(INGREDIENT_198)];
const PATTERN_182: &[Option<Ingredient>] = &[Some(INGREDIENT_201), Some(INGREDIENT_201), Some(INGREDIENT_201), Some(INGREDIENT_201), Some(INGREDIENT_201), Some(INGREDIENT_201)];
const PATTERN_183: &[Option<Ingredient>] = &[Some(INGREDIENT_201), Some(INGREDIENT_3), Some(INGREDIENT_201), Some(INGREDIENT_201), Some(INGREDIENT_3), Some(INGREDIENT_201)];
const PATTERN_184: &[Option<Ingredient>] = &[Some(INGREDIENT_3), Some(INGREDIENT_201), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_201), Some(INGREDIENT_3)];
const PATTERN_185: &[Option<Ingredient>] = &[Some(INGREDIENT_4), None, Some(INGREDIENT_4), Some(INGREDIENT_202), Some(INGREDIENT_202), Some(INGREDIENT_202), Some(INGREDIENT_202), Some(INGREDIENT_202), Some(INGREDIENT_202)];
const PATTERN_186: &[Option<Ingredient>] = &[Some(INGREDIENT_203), Some(INGREDIENT_203), Some(INGREDIENT_203), Some(INGREDIENT_203)];
const PATTERN_187: &[Option<Ingredient>] = &[Some(INGREDIENT_201), Some(INGREDIENT_201)];
const PATTERN_188: &[Option<Ingredient>] = &[Some(INGREDIENT_202), Some(INGREDIENT_202), Some(INGREDIENT_202), None, None, None, Some(INGREDIENT_202), Some(INGREDIENT_202), Some(INGREDIENT_202)];
const PATTERN_189: &[Option<Ingredient>] = &[Some(INGREDIENT_201), Some(INGREDIENT_201), Some(INGREDIENT_201), Some(INGREDIENT_201), Some(INGREDIENT_201), Some(INGREDIENT_201), None, Some(INGREDIENT_3), None];
const PATTERN_190: &[Option<Ingredient>] = &[Some(INGREDIENT_201), Some(INGREDIENT_201), Some(INGREDIENT_201)];
const PATTERN_191: &[Option<Ingredient>] = &[Some(INGREDIENT_201), None, None, Some(INGREDIENT_201), Some(INGREDIENT_201), None, Some(INGREDIENT_201), Some(INGREDIENT_201), Some(INGREDIENT_201)];
const PATTERN_192: &[Option<Ingredient>] = &[Some(INGREDIENT_3), Some(INGREDIENT_8), Some(INGREDIENT_3), Some(INGREDIENT_83), Some(INGREDIENT_205), Some(INGREDIENT_83), None, Some(INGREDIENT_3), None];
const PATTERN_193: &[Option<Ingredient>] = &[Some(INGREDIENT_139), Some(INGREDIENT_139), Some(INGREDIENT_139), Some(INGREDIENT_139)];
const PATTERN_194: &[Option<Ingredient>] = &[Some(INGREDIENT_140), Some(INGREDIENT_140), Some(INGREDIENT_140)];
const PATTERN_195: &[Option<Ingredient>] = &[Some(INGREDIENT_140), None, None, Some(INGREDIENT_140), Some(INGREDIENT_140), None, Some(INGREDIENT_140), Some(INGREDIENT_140), Some(INGREDIENT_140)];
const PATTERN_196: &[Option<Ingredient>] = &[Some(INGREDIENT_151), Some(INGREDIENT_151), Some(INGREDIENT_151), Some(INGREDIENT_151)];
const PATTERN_197: &[Option<Ingredient>] = &[Some(INGREDIENT_206), Some(INGREDIENT_206), Some(INGREDIENT_206)];
const PATTERN_198: &[Option<Ingredient>] = &[Some(INGREDIENT_155), Some(INGREDIENT_155), Some(INGREDIENT_155), Some(INGREDIENT_155)];
const PATTERN_199: &[Option<Ingredient>] = &[Some(INGREDIENT_207), Some(INGREDIENT_207), Some(INGREDIENT_207)];
const PATTERN_200: &[Option<Ingredient>] = &[Some(INGREDIENT_208), Some(INGREDIENT_208), Some(INGREDIENT_208), Some(INGREDIENT_208), Some(INGREDIENT_208), Some(INGREDIENT_208), None, Some(INGREDIENT_3), None];
const PATTERN_201: &[Option<Ingredient>] = &[Some(INGREDIENT_208), Some(INGREDIENT_208), Some(INGREDIENT_208), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_202: &[Option<Ingredient>] = &[Some(INGREDIENT_208), Some(INGREDIENT_208)];
const PATTERN_203: &[Option<Ingredient>] = &[Some(INGREDIENT_211), Some(INGREDIENT_211), Some(INGREDIENT_211)];
const PATTERN_204: &[Option<Ingredient>] = &[Some(INGREDIENT_211), None, None, Some(INGREDIENT_211), Some(INGREDIENT_211), None, Some(INGREDIENT_211), Some(INGREDIENT_211), Some(INGREDIENT_211)];
const PATTERN_205: &[Option<Ingredient>] = &[Some(INGREDIENT_212), Some(INGREDIENT_212), Some(INGREDIENT_212)];
const PATTERN_206: &[Option<Ingredient>] = &[Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_28), Some(INGREDIENT_208), Some(INGREDIENT_28)];
const PATTERN_207: &[Option<Ingredient>] = &[Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_210), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28)];
const PATTERN_208: &[Option<Ingredient>] = &[Some(INGREDIENT_216), Some(INGREDIENT_216), Some(INGREDIENT_216), Some(INGREDIENT_216), Some(INGREDIENT_216), Some(INGREDIENT_216)];
const PATTERN_209: &[Option<Ingredient>] = &[Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_210), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54)];
const PATTERN_210: &[Option<Ingredient>] = &[Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_210), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55)];
const PATTERN_211: &[Option<Ingredient>] = &[Some(INGREDIENT_208), Some(INGREDIENT_208), Some(INGREDIENT_208)];
const PATTERN_212: &[Option<Ingredient>] = &[Some(INGREDIENT_208), None, None, Some(INGREDIENT_208), Some(INGREDIENT_208), None, Some(INGREDIENT_208), Some(INGREDIENT_208), Some(INGREDIENT_208)];
const PATTERN_213: &[Option<Ingredient>] = &[Some(INGREDIENT_217), None, Some(INGREDIENT_217), Some(INGREDIENT_217), Some(INGREDIENT_217), Some(INGREDIENT_217)];
const PATTERN_214: &[Option<Ingredient>] = &[Some(INGREDIENT_217), Some(INGREDIENT_217), Some(INGREDIENT_217), Some(INGREDIENT_217), Some(INGREDIENT_217), Some(INGREDIENT_217)];
const PATTERN_215: &[Option<Ingredient>] = &[Some(INGREDIENT_217), Some(INGREDIENT_3), Some(INGREDIENT_217), Some(INGREDIENT_217), Some(INGREDIENT_3), Some(INGREDIENT_217)];
const PATTERN_216: &[Option<Ingredient>] = &[Some(INGREDIENT_3), Some(INGREDIENT_217), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_217), Some(INGREDIENT_3)];
const PATTERN_217: &[Option<Ingredient>] = &[Some(INGREDIENT_4), None, Some(INGREDIENT_4), Some(INGREDIENT_219), Some(INGREDIENT_219), Some(INGREDIENT_219), Some(INGREDIENT_219), Some(INGREDIENT_219), Some(INGREDIENT_219)];
const PATTERN_218: &[Option<Ingredient>] = &[Some(INGREDIENT_217), Some(INGREDIENT_217)];
const PATTERN_219: &[Option<Ingredient>] = &[Some(INGREDIENT_219), Some(INGREDIENT_219), Some(INGREDIENT_219), None, None, None, Some(INGREDIENT_219), Some(INGREDIENT_219), Some(INGREDIENT_219)];
const PATTERN_220: &[Option<Ingredient>] = &[Some(INGREDIENT_217), Some(INGREDIENT_217), Some(INGREDIENT_217), Some(INGREDIENT_217), Some(INGREDIENT_217), Some(INGREDIENT_217), None, Some(INGREDIENT_3), None];
const PATTERN_221: &[Option<Ingredient>] = &[Some(INGREDIENT_217), Some(INGREDIENT_217), Some(INGREDIENT_217)];
const PATTERN_222: &[Option<Ingredient>] = &[Some(INGREDIENT_217), None, None, Some(INGREDIENT_217), Some(INGREDIENT_217), None, Some(INGREDIENT_217), Some(INGREDIENT_217), Some(INGREDIENT_217)];
const PATTERN_223: &[Option<Ingredient>] = &[Some(INGREDIENT_221), Some(INGREDIENT_221), Some(INGREDIENT_221), Some(INGREDIENT_221)];
const PATTERN_224: &[Option<Ingredient>] = &[Some(INGREDIENT_222), Some(INGREDIENT_222), Some(INGREDIENT_222), Some(INGREDIENT_222), Some(INGREDIENT_42), Some(INGREDIENT_222), Some(INGREDIENT_222), Some(INGREDIENT_222), Some(INGREDIENT_222)];
const PATTERN_225: &[Option<Ingredient>] = &[Some(INGREDIENT_223), Some(INGREDIENT_223), Some(INGREDIENT_223)];
const PATTERN_226: &[Option<Ingredient>] = &[Some(INGREDIENT_223), None, None, Some(INGREDIENT_223), Some(INGREDIENT_223), None, Some(INGREDIENT_223), Some(INGREDIENT_223), Some(INGREDIENT_223)];
const PATTERN_227: &[Option<Ingredient>] = &[Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_174), Some(INGREDIENT_174), Some(INGREDIENT_174), Some(INGREDIENT_27), Some(INGREDIENT_27), Some(INGREDIENT_27)];
const PATTERN_228: &[Option<Ingredient>] = &[None, Some(INGREDIENT_112), None, Some(INGREDIENT_112), None, Some(INGREDIENT_112), None, Some(INGREDIENT_112), None];
const PATTERN_229: &[Option<Ingredient>] = &[Some(INGREDIENT_193), Some(INGREDIENT_193), Some(INGREDIENT_193)];
const PATTERN_230: &[Option<Ingredient>] = &[Some(INGREDIENT_193), None, None, Some(INGREDIENT_193), Some(INGREDIENT_193), None, Some(INGREDIENT_193), Some(INGREDIENT_193), Some(INGREDIENT_193)];
const PATTERN_231: &[Option<Ingredient>] = &[Some(INGREDIENT_193), Some(INGREDIENT_193), Some(INGREDIENT_193), Some(INGREDIENT_193), Some(INGREDIENT_193), Some(INGREDIENT_193)];
const PATTERN_232: &[Option<Ingredient>] = &[Some(INGREDIENT_225), Some(INGREDIENT_225), Some(INGREDIENT_225), Some(INGREDIENT_225)];
const PATTERN_233: &[Option<Ingredient>] = &[Some(INGREDIENT_194), Some(INGREDIENT_194), Some(INGREDIENT_194)];
const PATTERN_234: &[Option<Ingredient>] = &[Some(INGREDIENT_194), None, None, Some(INGREDIENT_194), Some(INGREDIENT_194), None, Some(INGREDIENT_194), Some(INGREDIENT_194), Some(INGREDIENT_194)];
const PATTERN_235: &[Option<Ingredient>] = &[Some(INGREDIENT_194), Some(INGREDIENT_194), Some(INGREDIENT_194), Some(INGREDIENT_194), Some(INGREDIENT_194), Some(INGREDIENT_194)];
const PATTERN_236: &[Option<Ingredient>] = &[Some(INGREDIENT_193), Some(INGREDIENT_193), Some(INGREDIENT_193), Some(INGREDIENT_193)];
const PATTERN_237: &[Option<Ingredient>] = &[Some(INGREDIENT_8), None, Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_226), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_101), Some(INGREDIENT_8)];
const PATTERN_238: &[Option<Ingredient>] = &[Some(INGREDIENT_228), Some(INGREDIENT_228), Some(INGREDIENT_228), Some(INGREDIENT_3), None, Some(INGREDIENT_3)];
const PATTERN_239: &[Option<Ingredient>] = &[Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70)];
const PATTERN_240: &[Option<Ingredient>] = &[Some(INGREDIENT_70), None, Some(INGREDIENT_70), Some(INGREDIENT_70), None, Some(INGREDIENT_70)];
const PATTERN_241: &[Option<Ingredient>] = &[Some(INGREDIENT_70), None, Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70)];
const PATTERN_242: &[Option<Ingredient>] = &[Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), None, Some(INGREDIENT_70)];
const PATTERN_243: &[Option<Ingredient>] = &[Some(INGREDIENT_228), Some(INGREDIENT_228), None, Some(INGREDIENT_3), None, Some(INGREDIENT_3)];
const PATTERN_244: &[Option<Ingredient>] = &[Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), None, Some(INGREDIENT_70), Some(INGREDIENT_70), None, Some(INGREDIENT_70)];
const PATTERN_245: &[Option<Ingredient>] = &[Some(INGREDIENT_228), Some(INGREDIENT_228), Some(INGREDIENT_228), None, Some(INGREDIENT_3), None, None, Some(INGREDIENT_3), None];
const PATTERN_246: &[Option<Ingredient>] = &[Some(INGREDIENT_228), Some(INGREDIENT_3), Some(INGREDIENT_3)];
const PATTERN_247: &[Option<Ingredient>] = &[None, None, Some(INGREDIENT_228), None, Some(INGREDIENT_3), None, Some(INGREDIENT_3), None, None];
const PATTERN_248: &[Option<Ingredient>] = &[Some(INGREDIENT_228), Some(INGREDIENT_228), Some(INGREDIENT_3)];
const PATTERN_249: &[Option<Ingredient>] = &[Some(INGREDIENT_12), Some(INGREDIENT_174), Some(INGREDIENT_174), Some(INGREDIENT_12)];
const PATTERN_250: &[Option<Ingredient>] = &[Some(INGREDIENT_11), Some(INGREDIENT_11), Some(INGREDIENT_11)];
const PATTERN_251: &[Option<Ingredient>] = &[Some(INGREDIENT_11), None, None, Some(INGREDIENT_11), Some(INGREDIENT_11), None, Some(INGREDIENT_11), Some(INGREDIENT_11), Some(INGREDIENT_11)];
const PATTERN_252: &[Option<Ingredient>] = &[Some(INGREDIENT_11), Some(INGREDIENT_11), Some(INGREDIENT_11), Some(INGREDIENT_11), Some(INGREDIENT_11), Some(INGREDIENT_11)];
const PATTERN_253: &[Option<Ingredient>] = &[Some(INGREDIENT_12), Some(INGREDIENT_12), Some(INGREDIENT_12), Some(INGREDIENT_12), Some(INGREDIENT_231), Some(INGREDIENT_12), Some(INGREDIENT_12), Some(INGREDIENT_101), Some(INGREDIENT_12)];
const PATTERN_254: &[Option<Ingredient>] = &[Some(INGREDIENT_89), Some(INGREDIENT_89), Some(INGREDIENT_89), Some(INGREDIENT_89), Some(INGREDIENT_232), Some(INGREDIENT_89), Some(INGREDIENT_89), Some(INGREDIENT_89), Some(INGREDIENT_89)];
const PATTERN_255: &[Option<Ingredient>] = &[Some(INGREDIENT_234), Some(INGREDIENT_234), Some(INGREDIENT_234), Some(INGREDIENT_234), Some(INGREDIENT_234), Some(INGREDIENT_234), Some(INGREDIENT_234), Some(INGREDIENT_234), Some(INGREDIENT_234)];
const PATTERN_256: &[Option<Ingredient>] = &[Some(INGREDIENT_236), Some(INGREDIENT_236), Some(INGREDIENT_236), Some(INGREDIENT_236)];
const PATTERN_257: &[Option<Ingredient>] = &[Some(INGREDIENT_12), Some(INGREDIENT_12), Some(INGREDIENT_12), Some(INGREDIENT_12), None, Some(INGREDIENT_12), Some(INGREDIENT_12), Some(INGREDIENT_101), Some(INGREDIENT_12)];
const PATTERN_258: &[Option<Ingredient>] = &[Some(INGREDIENT_70), Some(INGREDIENT_237), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_155), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70)];
const PATTERN_259: &[Option<Ingredient>] = &[Some(INGREDIENT_362), Some(INGREDIENT_362), Some(INGREDIENT_362), Some(INGREDIENT_362), Some(INGREDIENT_362), Some(INGREDIENT_362), Some(INGREDIENT_362), Some(INGREDIENT_362), Some(INGREDIENT_362)];
const PATTERN_260: &[Option<Ingredient>] = &[None, Some(INGREDIENT_81), None, Some(INGREDIENT_70), Some(INGREDIENT_30), Some(INGREDIENT_70), Some(INGREDIENT_30), Some(INGREDIENT_30), Some(INGREDIENT_30)];
const PATTERN_261: &[Option<Ingredient>] = &[Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_365), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_89), Some(INGREDIENT_28)];
const PATTERN_262: &[Option<Ingredient>] = &[Some(INGREDIENT_59), Some(INGREDIENT_366)];
const PATTERN_263: &[Option<Ingredient>] = &[Some(INGREDIENT_367), Some(INGREDIENT_367), Some(INGREDIENT_367)];
const PATTERN_264: &[Option<Ingredient>] = &[Some(INGREDIENT_367), None, None, Some(INGREDIENT_367), Some(INGREDIENT_367), None, Some(INGREDIENT_367), Some(INGREDIENT_367), Some(INGREDIENT_367)];
const PATTERN_265: &[Option<Ingredient>] = &[Some(INGREDIENT_367), Some(INGREDIENT_367), Some(INGREDIENT_367), Some(INGREDIENT_367), Some(INGREDIENT_367), Some(INGREDIENT_367)];
const PATTERN_266: &[Option<Ingredient>] = &[Some(INGREDIENT_368), Some(INGREDIENT_368), Some(INGREDIENT_368), Some(INGREDIENT_368)];
const PATTERN_267: &[Option<Ingredient>] = &[Some(INGREDIENT_30), Some(INGREDIENT_30), Some(INGREDIENT_30), Some(INGREDIENT_30), Some(INGREDIENT_365), Some(INGREDIENT_30), Some(INGREDIENT_30), Some(INGREDIENT_30), Some(INGREDIENT_30)];
const PATTERN_268: &[Option<Ingredient>] = &[Some(INGREDIENT_370), Some(INGREDIENT_370)];
const PATTERN_269: &[Option<Ingredient>] = &[None, Some(INGREDIENT_371), None, Some(INGREDIENT_371), Some(INGREDIENT_59), Some(INGREDIENT_371), None, Some(INGREDIENT_101), None];
const PATTERN_270: &[Option<Ingredient>] = &[None, Some(INGREDIENT_371), None, Some(INGREDIENT_371), None, Some(INGREDIENT_371), None, Some(INGREDIENT_371), None];
const PATTERN_271: &[Option<Ingredient>] = &[Some(INGREDIENT_371), Some(INGREDIENT_371), Some(INGREDIENT_371), Some(INGREDIENT_371)];
const PATTERN_272: &[Option<Ingredient>] = &[Some(INGREDIENT_372), Some(INGREDIENT_372), Some(INGREDIENT_372)];
const PATTERN_273: &[Option<Ingredient>] = &[Some(INGREDIENT_372), None, None, Some(INGREDIENT_372), Some(INGREDIENT_372), None, Some(INGREDIENT_372), Some(INGREDIENT_372), Some(INGREDIENT_372)];
const PATTERN_274: &[Option<Ingredient>] = &[Some(INGREDIENT_70), Some(INGREDIENT_373), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_368), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70)];
const PATTERN_275: &[Option<Ingredient>] = &[None, None, Some(INGREDIENT_3), None, Some(INGREDIENT_3), Some(INGREDIENT_83), Some(INGREDIENT_3), None, Some(INGREDIENT_83)];
const PATTERN_276: &[Option<Ingredient>] = &[Some(INGREDIENT_16), Some(INGREDIENT_16), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_277: &[Option<Ingredient>] = &[Some(INGREDIENT_70), Some(INGREDIENT_380), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_87), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70)];
const PATTERN_278: &[Option<Ingredient>] = &[Some(INGREDIENT_112), None, Some(INGREDIENT_112), None, Some(INGREDIENT_112), None];
const PATTERN_279: &[Option<Ingredient>] = &[Some(INGREDIENT_109), Some(INGREDIENT_109), Some(INGREDIENT_109), Some(INGREDIENT_109), None, Some(INGREDIENT_109), Some(INGREDIENT_109), Some(INGREDIENT_109), Some(INGREDIENT_109)];
const PATTERN_280: &[Option<Ingredient>] = &[Some(INGREDIENT_28), None, Some(INGREDIENT_28), None, Some(INGREDIENT_28), None];
const PATTERN_281: &[Option<Ingredient>] = &[Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28)];
const PATTERN_282: &[Option<Ingredient>] = &[Some(INGREDIENT_378), Some(INGREDIENT_378), Some(INGREDIENT_378), Some(INGREDIENT_378), Some(INGREDIENT_383), Some(INGREDIENT_378), Some(INGREDIENT_378), Some(INGREDIENT_378), Some(INGREDIENT_378)];
const PATTERN_283: &[Option<Ingredient>] = &[Some(INGREDIENT_102), Some(INGREDIENT_102), Some(INGREDIENT_102), Some(INGREDIENT_102)];
const PATTERN_284: &[Option<Ingredient>] = &[Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_167)];
const PATTERN_285: &[Option<Ingredient>] = &[Some(INGREDIENT_378), Some(INGREDIENT_378), Some(INGREDIENT_378), Some(INGREDIENT_378), Some(INGREDIENT_378), Some(INGREDIENT_378), Some(INGREDIENT_378), Some(INGREDIENT_378), Some(INGREDIENT_378)];
const PATTERN_286: &[Option<Ingredient>] = &[Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_392), Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_167)];
const PATTERN_287: &[Option<Ingredient>] = &[Some(INGREDIENT_393), Some(INGREDIENT_393), Some(INGREDIENT_393), Some(INGREDIENT_3), None, Some(INGREDIENT_3)];
const PATTERN_288: &[Option<Ingredient>] = &[Some(INGREDIENT_167), None, Some(INGREDIENT_167), Some(INGREDIENT_167), None, Some(INGREDIENT_167)];
const PATTERN_289: &[Option<Ingredient>] = &[Some(INGREDIENT_378), Some(INGREDIENT_378), Some(INGREDIENT_378), Some(INGREDIENT_378), Some(INGREDIENT_128), Some(INGREDIENT_378), Some(INGREDIENT_378), Some(INGREDIENT_378), Some(INGREDIENT_378)];
const PATTERN_290: &[Option<Ingredient>] = &[Some(INGREDIENT_167), None, Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_167)];
const PATTERN_291: &[Option<Ingredient>] = &[Some(INGREDIENT_378), Some(INGREDIENT_378), Some(INGREDIENT_378), Some(INGREDIENT_378), Some(INGREDIENT_394), Some(INGREDIENT_378), Some(INGREDIENT_378), Some(INGREDIENT_378), Some(INGREDIENT_378)];
const PATTERN_292: &[Option<Ingredient>] = &[Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_167), None, Some(INGREDIENT_167)];
const PATTERN_293: &[Option<Ingredient>] = &[Some(INGREDIENT_393), Some(INGREDIENT_393), None, Some(INGREDIENT_3), None, Some(INGREDIENT_3)];
const PATTERN_294: &[Option<Ingredient>] = &[Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_167), None, Some(INGREDIENT_167), Some(INGREDIENT_167), None, Some(INGREDIENT_167)];
const PATTERN_295: &[Option<Ingredient>] = &[Some(INGREDIENT_393), Some(INGREDIENT_393), Some(INGREDIENT_393), None, Some(INGREDIENT_3), None, None, Some(INGREDIENT_3), None];
const PATTERN_296: &[Option<Ingredient>] = &[Some(INGREDIENT_393), Some(INGREDIENT_3), Some(INGREDIENT_3)];
const PATTERN_297: &[Option<Ingredient>] = &[None, None, Some(INGREDIENT_393), None, Some(INGREDIENT_3), None, Some(INGREDIENT_3), None, None];
const PATTERN_298: &[Option<Ingredient>] = &[Some(INGREDIENT_393), Some(INGREDIENT_393), Some(INGREDIENT_3)];
const PATTERN_299: &[Option<Ingredient>] = &[Some(INGREDIENT_395), Some(INGREDIENT_395), Some(INGREDIENT_395)];
const PATTERN_300: &[Option<Ingredient>] = &[Some(INGREDIENT_395), None, None, Some(INGREDIENT_395), Some(INGREDIENT_395), None, Some(INGREDIENT_395), Some(INGREDIENT_395), Some(INGREDIENT_395)];
const PATTERN_301: &[Option<Ingredient>] = &[Some(INGREDIENT_395), Some(INGREDIENT_395), Some(INGREDIENT_395), Some(INGREDIENT_395), Some(INGREDIENT_395), Some(INGREDIENT_395)];
const PATTERN_302: &[Option<Ingredient>] = &[Some(INGREDIENT_396), Some(INGREDIENT_396), Some(INGREDIENT_396), Some(INGREDIENT_396), Some(INGREDIENT_396), Some(INGREDIENT_396), None, Some(INGREDIENT_3), None];
const PATTERN_303: &[Option<Ingredient>] = &[Some(INGREDIENT_396), Some(INGREDIENT_396), Some(INGREDIENT_396), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_304: &[Option<Ingredient>] = &[Some(INGREDIENT_396), Some(INGREDIENT_396)];
const PATTERN_305: &[Option<Ingredient>] = &[Some(INGREDIENT_398), Some(INGREDIENT_398), Some(INGREDIENT_398)];
const PATTERN_306: &[Option<Ingredient>] = &[Some(INGREDIENT_398), None, None, Some(INGREDIENT_398), Some(INGREDIENT_398), None, Some(INGREDIENT_398), Some(INGREDIENT_398), Some(INGREDIENT_398)];
const PATTERN_307: &[Option<Ingredient>] = &[Some(INGREDIENT_399), Some(INGREDIENT_399), Some(INGREDIENT_399)];
const PATTERN_308: &[Option<Ingredient>] = &[Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_28), Some(INGREDIENT_396), Some(INGREDIENT_28)];
const PATTERN_309: &[Option<Ingredient>] = &[Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_266), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28)];
const PATTERN_310: &[Option<Ingredient>] = &[Some(INGREDIENT_402), Some(INGREDIENT_402), Some(INGREDIENT_402), Some(INGREDIENT_402), Some(INGREDIENT_402), Some(INGREDIENT_402)];
const PATTERN_311: &[Option<Ingredient>] = &[Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_266), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54)];
const PATTERN_312: &[Option<Ingredient>] = &[Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_266), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55)];
const PATTERN_313: &[Option<Ingredient>] = &[Some(INGREDIENT_396), Some(INGREDIENT_396), Some(INGREDIENT_396)];
const PATTERN_314: &[Option<Ingredient>] = &[Some(INGREDIENT_396), None, None, Some(INGREDIENT_396), Some(INGREDIENT_396), None, Some(INGREDIENT_396), Some(INGREDIENT_396), Some(INGREDIENT_396)];
const PATTERN_315: &[Option<Ingredient>] = &[Some(INGREDIENT_403), Some(INGREDIENT_403), Some(INGREDIENT_403), Some(INGREDIENT_403), Some(INGREDIENT_403), Some(INGREDIENT_403), None, Some(INGREDIENT_3), None];
const PATTERN_316: &[Option<Ingredient>] = &[Some(INGREDIENT_403), Some(INGREDIENT_403), Some(INGREDIENT_403), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_317: &[Option<Ingredient>] = &[Some(INGREDIENT_403), Some(INGREDIENT_403)];
const PATTERN_318: &[Option<Ingredient>] = &[Some(INGREDIENT_405), Some(INGREDIENT_405), Some(INGREDIENT_405)];
const PATTERN_319: &[Option<Ingredient>] = &[Some(INGREDIENT_405), None, None, Some(INGREDIENT_405), Some(INGREDIENT_405), None, Some(INGREDIENT_405), Some(INGREDIENT_405), Some(INGREDIENT_405)];
const PATTERN_320: &[Option<Ingredient>] = &[Some(INGREDIENT_406), Some(INGREDIENT_406), Some(INGREDIENT_406)];
const PATTERN_321: &[Option<Ingredient>] = &[Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_28), Some(INGREDIENT_403), Some(INGREDIENT_28)];
const PATTERN_322: &[Option<Ingredient>] = &[Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_213), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28)];
const PATTERN_323: &[Option<Ingredient>] = &[Some(INGREDIENT_409), Some(INGREDIENT_409), Some(INGREDIENT_409), Some(INGREDIENT_409), Some(INGREDIENT_409), Some(INGREDIENT_409)];
const PATTERN_324: &[Option<Ingredient>] = &[Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_213), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54)];
const PATTERN_325: &[Option<Ingredient>] = &[Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_213), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55)];
const PATTERN_326: &[Option<Ingredient>] = &[Some(INGREDIENT_403), Some(INGREDIENT_403), Some(INGREDIENT_403)];
const PATTERN_327: &[Option<Ingredient>] = &[Some(INGREDIENT_403), None, None, Some(INGREDIENT_403), Some(INGREDIENT_403), None, Some(INGREDIENT_403), Some(INGREDIENT_403), Some(INGREDIENT_403)];
const PATTERN_328: &[Option<Ingredient>] = &[Some(INGREDIENT_3), Some(INGREDIENT_410), Some(INGREDIENT_3), Some(INGREDIENT_26), None, Some(INGREDIENT_26)];
const PATTERN_329: &[Option<Ingredient>] = &[Some(INGREDIENT_8), Some(INGREDIENT_8)];
const PATTERN_330: &[Option<Ingredient>] = &[Some(INGREDIENT_411), Some(INGREDIENT_411), Some(INGREDIENT_411), Some(INGREDIENT_411)];
const PATTERN_331: &[Option<Ingredient>] = &[Some(INGREDIENT_31), Some(INGREDIENT_31), Some(INGREDIENT_31), Some(INGREDIENT_31)];
const PATTERN_332: &[Option<Ingredient>] = &[Some(INGREDIENT_8), None, Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_1), Some(INGREDIENT_8), None, Some(INGREDIENT_8), None];
const PATTERN_333: &[Option<Ingredient>] = &[Some(INGREDIENT_70), Some(INGREDIENT_415), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_55), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70)];
const PATTERN_334: &[Option<Ingredient>] = &[Some(INGREDIENT_416), Some(INGREDIENT_416), Some(INGREDIENT_416), Some(INGREDIENT_3), None, Some(INGREDIENT_3)];
const PATTERN_335: &[Option<Ingredient>] = &[Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8)];
const PATTERN_336: &[Option<Ingredient>] = &[Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8)];
const PATTERN_337: &[Option<Ingredient>] = &[Some(INGREDIENT_8), None, Some(INGREDIENT_8), Some(INGREDIENT_8), None, Some(INGREDIENT_8)];
const PATTERN_338: &[Option<Ingredient>] = &[Some(INGREDIENT_417), Some(INGREDIENT_8), Some(INGREDIENT_417)];
const PATTERN_339: &[Option<Ingredient>] = &[Some(INGREDIENT_8), None, Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8)];
const PATTERN_340: &[Option<Ingredient>] = &[Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), None, Some(INGREDIENT_8)];
const PATTERN_341: &[Option<Ingredient>] = &[Some(INGREDIENT_416), Some(INGREDIENT_416), None, Some(INGREDIENT_3), None, Some(INGREDIENT_3)];
const PATTERN_342: &[Option<Ingredient>] = &[Some(INGREDIENT_417), Some(INGREDIENT_417), Some(INGREDIENT_417), Some(INGREDIENT_417), Some(INGREDIENT_417), Some(INGREDIENT_417), Some(INGREDIENT_417), Some(INGREDIENT_417), Some(INGREDIENT_417)];
const PATTERN_343: &[Option<Ingredient>] = &[Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), None, Some(INGREDIENT_8), Some(INGREDIENT_8), None, Some(INGREDIENT_8)];
const PATTERN_344: &[Option<Ingredient>] = &[Some(INGREDIENT_416), Some(INGREDIENT_416), Some(INGREDIENT_416), None, Some(INGREDIENT_3), None, None, Some(INGREDIENT_3), None];
const PATTERN_345: &[Option<Ingredient>] = &[Some(INGREDIENT_416), Some(INGREDIENT_3), Some(INGREDIENT_3)];
const PATTERN_346: &[Option<Ingredient>] = &[None, None, Some(INGREDIENT_416), None, Some(INGREDIENT_3), None, Some(INGREDIENT_3), None, None];
const PATTERN_347: &[Option<Ingredient>] = &[Some(INGREDIENT_416), Some(INGREDIENT_416), Some(INGREDIENT_3)];
const PATTERN_348: &[Option<Ingredient>] = &[Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8)];
const PATTERN_349: &[Option<Ingredient>] = &[Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_51), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_3)];
const PATTERN_350: &[Option<Ingredient>] = &[Some(INGREDIENT_422), Some(INGREDIENT_423)];
const PATTERN_351: &[Option<Ingredient>] = &[Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_70), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_352: &[Option<Ingredient>] = &[Some(INGREDIENT_424), None, Some(INGREDIENT_424), Some(INGREDIENT_424), Some(INGREDIENT_424), Some(INGREDIENT_424)];
const PATTERN_353: &[Option<Ingredient>] = &[Some(INGREDIENT_424), Some(INGREDIENT_424), Some(INGREDIENT_424), Some(INGREDIENT_424), Some(INGREDIENT_424), Some(INGREDIENT_424)];
const PATTERN_354: &[Option<Ingredient>] = &[Some(INGREDIENT_424), Some(INGREDIENT_3), Some(INGREDIENT_424), Some(INGREDIENT_424), Some(INGREDIENT_3), Some(INGREDIENT_424)];
const PATTERN_355: &[Option<Ingredient>] = &[Some(INGREDIENT_3), Some(INGREDIENT_424), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_424), Some(INGREDIENT_3)];
const PATTERN_356: &[Option<Ingredient>] = &[Some(INGREDIENT_4), None, Some(INGREDIENT_4), Some(INGREDIENT_426), Some(INGREDIENT_426), Some(INGREDIENT_426), Some(INGREDIENT_426), Some(INGREDIENT_426), Some(INGREDIENT_426)];
const PATTERN_357: &[Option<Ingredient>] = &[Some(INGREDIENT_424), Some(INGREDIENT_424)];
const PATTERN_358: &[Option<Ingredient>] = &[Some(INGREDIENT_426), Some(INGREDIENT_426), Some(INGREDIENT_426), None, None, None, Some(INGREDIENT_426), Some(INGREDIENT_426), Some(INGREDIENT_426)];
const PATTERN_359: &[Option<Ingredient>] = &[Some(INGREDIENT_424), Some(INGREDIENT_424), Some(INGREDIENT_424), Some(INGREDIENT_424), Some(INGREDIENT_424), Some(INGREDIENT_424), None, Some(INGREDIENT_3), None];
const PATTERN_360: &[Option<Ingredient>] = &[Some(INGREDIENT_424), Some(INGREDIENT_424), Some(INGREDIENT_424)];
const PATTERN_361: &[Option<Ingredient>] = &[Some(INGREDIENT_424), None, None, Some(INGREDIENT_424), Some(INGREDIENT_424), None, Some(INGREDIENT_424), Some(INGREDIENT_424), Some(INGREDIENT_424)];
const PATTERN_362: &[Option<Ingredient>] = &[Some(INGREDIENT_428), Some(INGREDIENT_428), Some(INGREDIENT_428), Some(INGREDIENT_428)];
const PATTERN_363: &[Option<Ingredient>] = &[Some(INGREDIENT_3), None, Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_3), None, Some(INGREDIENT_3)];
const PATTERN_364: &[Option<Ingredient>] = &[Some(INGREDIENT_417), Some(INGREDIENT_417), Some(INGREDIENT_417), Some(INGREDIENT_417), Some(INGREDIENT_423), Some(INGREDIENT_417), Some(INGREDIENT_417), Some(INGREDIENT_417), Some(INGREDIENT_417)];
const PATTERN_365: &[Option<Ingredient>] = &[Some(INGREDIENT_65), Some(INGREDIENT_65), Some(INGREDIENT_65), Some(INGREDIENT_65), Some(INGREDIENT_65), Some(INGREDIENT_65), Some(INGREDIENT_65), Some(INGREDIENT_65), Some(INGREDIENT_65)];
const PATTERN_366: &[Option<Ingredient>] = &[Some(INGREDIENT_83), Some(INGREDIENT_83), None, Some(INGREDIENT_83), Some(INGREDIENT_83), None, None, None, Some(INGREDIENT_83)];
const PATTERN_367: &[Option<Ingredient>] = &[Some(INGREDIENT_433), Some(INGREDIENT_433), Some(INGREDIENT_433), Some(INGREDIENT_433)];
const PATTERN_368: &[Option<Ingredient>] = &[Some(INGREDIENT_51), None, Some(INGREDIENT_51), Some(INGREDIENT_51), None, Some(INGREDIENT_51)];
const PATTERN_369: &[Option<Ingredient>] = &[Some(INGREDIENT_51), None, Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51)];
const PATTERN_370: &[Option<Ingredient>] = &[Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), None, Some(INGREDIENT_51)];
const PATTERN_371: &[Option<Ingredient>] = &[Some(INGREDIENT_51), None, Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), None, Some(INGREDIENT_51)];
const PATTERN_372: &[Option<Ingredient>] = &[Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), None, Some(INGREDIENT_51), Some(INGREDIENT_51), None, Some(INGREDIENT_51)];
const PATTERN_373: &[Option<Ingredient>] = &[Some(INGREDIENT_27), Some(INGREDIENT_27), Some(INGREDIENT_27), None, Some(INGREDIENT_439), None, None, Some(INGREDIENT_27), None];
const PATTERN_374: &[Option<Ingredient>] = &[Some(INGREDIENT_3), Some(INGREDIENT_12)];
const PATTERN_375: &[Option<Ingredient>] = &[Some(INGREDIENT_440), Some(INGREDIENT_440), Some(INGREDIENT_440), Some(INGREDIENT_440), Some(INGREDIENT_440), Some(INGREDIENT_440), None, Some(INGREDIENT_3), None];
const PATTERN_376: &[Option<Ingredient>] = &[Some(INGREDIENT_440), Some(INGREDIENT_440), Some(INGREDIENT_440), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_377: &[Option<Ingredient>] = &[Some(INGREDIENT_440), Some(INGREDIENT_440)];
const PATTERN_378: &[Option<Ingredient>] = &[Some(INGREDIENT_442), Some(INGREDIENT_442), Some(INGREDIENT_442)];
const PATTERN_379: &[Option<Ingredient>] = &[Some(INGREDIENT_442), None, None, Some(INGREDIENT_442), Some(INGREDIENT_442), None, Some(INGREDIENT_442), Some(INGREDIENT_442), Some(INGREDIENT_442)];
const PATTERN_380: &[Option<Ingredient>] = &[Some(INGREDIENT_443), Some(INGREDIENT_443), Some(INGREDIENT_443)];
const PATTERN_381: &[Option<Ingredient>] = &[Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_28), Some(INGREDIENT_440), Some(INGREDIENT_28)];
const PATTERN_382: &[Option<Ingredient>] = &[Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_281), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28)];
const PATTERN_383: &[Option<Ingredient>] = &[Some(INGREDIENT_446), Some(INGREDIENT_446), Some(INGREDIENT_446), Some(INGREDIENT_446), Some(INGREDIENT_446), Some(INGREDIENT_446)];
const PATTERN_384: &[Option<Ingredient>] = &[Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_281), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54)];
const PATTERN_385: &[Option<Ingredient>] = &[Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_281), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55)];
const PATTERN_386: &[Option<Ingredient>] = &[Some(INGREDIENT_440), Some(INGREDIENT_440), Some(INGREDIENT_440)];
const PATTERN_387: &[Option<Ingredient>] = &[Some(INGREDIENT_440), None, None, Some(INGREDIENT_440), Some(INGREDIENT_440), None, Some(INGREDIENT_440), Some(INGREDIENT_440), Some(INGREDIENT_440)];
const PATTERN_388: &[Option<Ingredient>] = &[Some(INGREDIENT_447), Some(INGREDIENT_447), Some(INGREDIENT_447), Some(INGREDIENT_447), Some(INGREDIENT_447), Some(INGREDIENT_447), None, Some(INGREDIENT_3), None];
const PATTERN_389: &[Option<Ingredient>] = &[Some(INGREDIENT_447), Some(INGREDIENT_447), Some(INGREDIENT_447), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_390: &[Option<Ingredient>] = &[Some(INGREDIENT_447), Some(INGREDIENT_447)];
const PATTERN_391: &[Option<Ingredient>] = &[Some(INGREDIENT_449), Some(INGREDIENT_449), Some(INGREDIENT_449)];
const PATTERN_392: &[Option<Ingredient>] = &[Some(INGREDIENT_449), None, None, Some(INGREDIENT_449), Some(INGREDIENT_449), None, Some(INGREDIENT_449), Some(INGREDIENT_449), Some(INGREDIENT_449)];
const PATTERN_393: &[Option<Ingredient>] = &[Some(INGREDIENT_450), Some(INGREDIENT_450), Some(INGREDIENT_450)];
const PATTERN_394: &[Option<Ingredient>] = &[Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_28), Some(INGREDIENT_447), Some(INGREDIENT_28)];
const PATTERN_395: &[Option<Ingredient>] = &[Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_289), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28)];
const PATTERN_396: &[Option<Ingredient>] = &[Some(INGREDIENT_454), Some(INGREDIENT_454), Some(INGREDIENT_454), Some(INGREDIENT_454), Some(INGREDIENT_454), Some(INGREDIENT_454)];
const PATTERN_397: &[Option<Ingredient>] = &[Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_289), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54)];
const PATTERN_398: &[Option<Ingredient>] = &[Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_289), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55)];
const PATTERN_399: &[Option<Ingredient>] = &[Some(INGREDIENT_447), Some(INGREDIENT_447), Some(INGREDIENT_447)];
const PATTERN_400: &[Option<Ingredient>] = &[Some(INGREDIENT_447), None, None, Some(INGREDIENT_447), Some(INGREDIENT_447), None, Some(INGREDIENT_447), Some(INGREDIENT_447), Some(INGREDIENT_447)];
const PATTERN_401: &[Option<Ingredient>] = &[Some(INGREDIENT_167), Some(INGREDIENT_167)];
const PATTERN_402: &[Option<Ingredient>] = &[Some(INGREDIENT_121), Some(INGREDIENT_121), Some(INGREDIENT_121)];
const PATTERN_403: &[Option<Ingredient>] = &[Some(INGREDIENT_455), Some(INGREDIENT_455), Some(INGREDIENT_455), Some(INGREDIENT_455), Some(INGREDIENT_455), Some(INGREDIENT_455), None, Some(INGREDIENT_3), None];
const PATTERN_404: &[Option<Ingredient>] = &[Some(INGREDIENT_455), Some(INGREDIENT_455), Some(INGREDIENT_455), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_405: &[Option<Ingredient>] = &[Some(INGREDIENT_455), Some(INGREDIENT_455)];
const PATTERN_406: &[Option<Ingredient>] = &[Some(INGREDIENT_457), Some(INGREDIENT_457), Some(INGREDIENT_457)];
const PATTERN_407: &[Option<Ingredient>] = &[Some(INGREDIENT_457), None, None, Some(INGREDIENT_457), Some(INGREDIENT_457), None, Some(INGREDIENT_457), Some(INGREDIENT_457), Some(INGREDIENT_457)];
const PATTERN_408: &[Option<Ingredient>] = &[Some(INGREDIENT_458), Some(INGREDIENT_458), Some(INGREDIENT_458)];
const PATTERN_409: &[Option<Ingredient>] = &[Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_28), Some(INGREDIENT_455), Some(INGREDIENT_28)];
const PATTERN_410: &[Option<Ingredient>] = &[Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_297), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28)];
const PATTERN_411: &[Option<Ingredient>] = &[Some(INGREDIENT_461), Some(INGREDIENT_461), Some(INGREDIENT_461), Some(INGREDIENT_461), Some(INGREDIENT_461), Some(INGREDIENT_461)];
const PATTERN_412: &[Option<Ingredient>] = &[Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_297), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54)];
const PATTERN_413: &[Option<Ingredient>] = &[Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_297), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55)];
const PATTERN_414: &[Option<Ingredient>] = &[Some(INGREDIENT_455), Some(INGREDIENT_455), Some(INGREDIENT_455)];
const PATTERN_415: &[Option<Ingredient>] = &[Some(INGREDIENT_455), None, None, Some(INGREDIENT_455), Some(INGREDIENT_455), None, Some(INGREDIENT_455), Some(INGREDIENT_455), Some(INGREDIENT_455)];
const PATTERN_416: &[Option<Ingredient>] = &[Some(INGREDIENT_462), Some(INGREDIENT_462), Some(INGREDIENT_462), Some(INGREDIENT_462), Some(INGREDIENT_8), Some(INGREDIENT_462), Some(INGREDIENT_462), Some(INGREDIENT_462), Some(INGREDIENT_462)];
const PATTERN_417: &[Option<Ingredient>] = &[Some(INGREDIENT_83), Some(INGREDIENT_83), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_418: &[Option<Ingredient>] = &[Some(INGREDIENT_463), Some(INGREDIENT_87)];
const PATTERN_419: &[Option<Ingredient>] = &[Some(INGREDIENT_464), Some(INGREDIENT_464), Some(INGREDIENT_464), Some(INGREDIENT_464), Some(INGREDIENT_464), Some(INGREDIENT_464), None, Some(INGREDIENT_3), None];
const PATTERN_420: &[Option<Ingredient>] = &[Some(INGREDIENT_464), Some(INGREDIENT_464), Some(INGREDIENT_464), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_421: &[Option<Ingredient>] = &[Some(INGREDIENT_464), Some(INGREDIENT_464)];
const PATTERN_422: &[Option<Ingredient>] = &[Some(INGREDIENT_466), Some(INGREDIENT_466), Some(INGREDIENT_466)];
const PATTERN_423: &[Option<Ingredient>] = &[Some(INGREDIENT_466), None, None, Some(INGREDIENT_466), Some(INGREDIENT_466), None, Some(INGREDIENT_466), Some(INGREDIENT_466), Some(INGREDIENT_466)];
const PATTERN_424: &[Option<Ingredient>] = &[Some(INGREDIENT_467), Some(INGREDIENT_467), Some(INGREDIENT_467)];
const PATTERN_425: &[Option<Ingredient>] = &[Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_28), Some(INGREDIENT_464), Some(INGREDIENT_28)];
const PATTERN_426: &[Option<Ingredient>] = &[Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_305), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28)];
const PATTERN_427: &[Option<Ingredient>] = &[Some(INGREDIENT_471), Some(INGREDIENT_471), Some(INGREDIENT_471), Some(INGREDIENT_471), Some(INGREDIENT_471), Some(INGREDIENT_471)];
const PATTERN_428: &[Option<Ingredient>] = &[Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_305), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54)];
const PATTERN_429: &[Option<Ingredient>] = &[Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_305), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55)];
const PATTERN_430: &[Option<Ingredient>] = &[Some(INGREDIENT_464), Some(INGREDIENT_464), Some(INGREDIENT_464)];
const PATTERN_431: &[Option<Ingredient>] = &[Some(INGREDIENT_464), None, None, Some(INGREDIENT_464), Some(INGREDIENT_464), None, Some(INGREDIENT_464), Some(INGREDIENT_464), Some(INGREDIENT_464)];
const PATTERN_432: &[Option<Ingredient>] = &[Some(INGREDIENT_92), Some(INGREDIENT_92), Some(INGREDIENT_92), Some(INGREDIENT_92)];
const PATTERN_433: &[Option<Ingredient>] = &[Some(INGREDIENT_473), None, Some(INGREDIENT_473), Some(INGREDIENT_473), Some(INGREDIENT_473), Some(INGREDIENT_473)];
const PATTERN_434: &[Option<Ingredient>] = &[Some(INGREDIENT_473), Some(INGREDIENT_473), Some(INGREDIENT_473), Some(INGREDIENT_473), Some(INGREDIENT_473), Some(INGREDIENT_473)];
const PATTERN_435: &[Option<Ingredient>] = &[Some(INGREDIENT_473), Some(INGREDIENT_3), Some(INGREDIENT_473), Some(INGREDIENT_473), Some(INGREDIENT_3), Some(INGREDIENT_473)];
const PATTERN_436: &[Option<Ingredient>] = &[Some(INGREDIENT_3), Some(INGREDIENT_473), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_473), Some(INGREDIENT_3)];
const PATTERN_437: &[Option<Ingredient>] = &[Some(INGREDIENT_4), None, Some(INGREDIENT_4), Some(INGREDIENT_475), Some(INGREDIENT_475), Some(INGREDIENT_475), Some(INGREDIENT_475), Some(INGREDIENT_475), Some(INGREDIENT_475)];
const PATTERN_438: &[Option<Ingredient>] = &[Some(INGREDIENT_473), Some(INGREDIENT_473)];
const PATTERN_439: &[Option<Ingredient>] = &[Some(INGREDIENT_475), Some(INGREDIENT_475), Some(INGREDIENT_475), None, None, None, Some(INGREDIENT_475), Some(INGREDIENT_475), Some(INGREDIENT_475)];
const PATTERN_440: &[Option<Ingredient>] = &[Some(INGREDIENT_473), Some(INGREDIENT_473), Some(INGREDIENT_473), Some(INGREDIENT_473), Some(INGREDIENT_473), Some(INGREDIENT_473), None, Some(INGREDIENT_3), None];
const PATTERN_441: &[Option<Ingredient>] = &[Some(INGREDIENT_473), Some(INGREDIENT_473), Some(INGREDIENT_473)];
const PATTERN_442: &[Option<Ingredient>] = &[Some(INGREDIENT_473), None, None, Some(INGREDIENT_473), Some(INGREDIENT_473), None, Some(INGREDIENT_473), Some(INGREDIENT_473), Some(INGREDIENT_473)];
const PATTERN_443: &[Option<Ingredient>] = &[Some(INGREDIENT_477), Some(INGREDIENT_477), Some(INGREDIENT_477), Some(INGREDIENT_477)];
const PATTERN_444: &[Option<Ingredient>] = &[Some(INGREDIENT_78), Some(INGREDIENT_78), Some(INGREDIENT_78), Some(INGREDIENT_78), Some(INGREDIENT_478), Some(INGREDIENT_78), Some(INGREDIENT_78), Some(INGREDIENT_78), Some(INGREDIENT_78)];
const PATTERN_445: &[Option<Ingredient>] = &[Some(INGREDIENT_8), None, Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_8)];
const PATTERN_446: &[Option<Ingredient>] = &[Some(INGREDIENT_483), Some(INGREDIENT_483)];
const PATTERN_447: &[Option<Ingredient>] = &[Some(INGREDIENT_484), Some(INGREDIENT_484), Some(INGREDIENT_484)];
const PATTERN_448: &[Option<Ingredient>] = &[Some(INGREDIENT_484), None, None, Some(INGREDIENT_484), Some(INGREDIENT_484), None, Some(INGREDIENT_484), Some(INGREDIENT_484), Some(INGREDIENT_484)];
const PATTERN_449: &[Option<Ingredient>] = &[Some(INGREDIENT_484), Some(INGREDIENT_484), Some(INGREDIENT_484), Some(INGREDIENT_484), Some(INGREDIENT_484), Some(INGREDIENT_484)];
const PATTERN_450: &[Option<Ingredient>] = &[Some(INGREDIENT_485), Some(INGREDIENT_485), Some(INGREDIENT_485)];
const PATTERN_451: &[Option<Ingredient>] = &[Some(INGREDIENT_485), None, None, Some(INGREDIENT_485), Some(INGREDIENT_485), None, Some(INGREDIENT_485), Some(INGREDIENT_485), Some(INGREDIENT_485)];
const PATTERN_452: &[Option<Ingredient>] = &[Some(INGREDIENT_485), Some(INGREDIENT_485), Some(INGREDIENT_485), Some(INGREDIENT_485), Some(INGREDIENT_485), Some(INGREDIENT_485)];
const PATTERN_453: &[Option<Ingredient>] = &[Some(INGREDIENT_486), Some(INGREDIENT_486), Some(INGREDIENT_486)];
const PATTERN_454: &[Option<Ingredient>] = &[Some(INGREDIENT_486), None, None, Some(INGREDIENT_486), Some(INGREDIENT_486), None, Some(INGREDIENT_486), Some(INGREDIENT_486), Some(INGREDIENT_486)];
const PATTERN_455: &[Option<Ingredient>] = &[Some(INGREDIENT_486), Some(INGREDIENT_486), Some(INGREDIENT_486), Some(INGREDIENT_486), Some(INGREDIENT_486), Some(INGREDIENT_486)];
const PATTERN_456: &[Option<Ingredient>] = &[Some(INGREDIENT_487), Some(INGREDIENT_487), Some(INGREDIENT_487), Some(INGREDIENT_487)];
const PATTERN_457: &[Option<Ingredient>] = &[None, Some(INGREDIENT_492), Some(INGREDIENT_78), None];
const PATTERN_458: &[Option<Ingredient>] = &[Some(INGREDIENT_145), Some(INGREDIENT_494), Some(INGREDIENT_145), Some(INGREDIENT_145), Some(INGREDIENT_494), Some(INGREDIENT_145)];
const PATTERN_459: &[Option<Ingredient>] = &[Some(INGREDIENT_145), Some(INGREDIENT_145), Some(INGREDIENT_145)];
const PATTERN_460: &[Option<Ingredient>] = &[Some(INGREDIENT_145), None, None, Some(INGREDIENT_145), Some(INGREDIENT_145), None, Some(INGREDIENT_145), Some(INGREDIENT_145), Some(INGREDIENT_145)];
const PATTERN_461: &[Option<Ingredient>] = &[Some(INGREDIENT_145), Some(INGREDIENT_145), Some(INGREDIENT_145), Some(INGREDIENT_145), Some(INGREDIENT_145), Some(INGREDIENT_145)];
const PATTERN_462: &[Option<Ingredient>] = &[Some(INGREDIENT_494), Some(INGREDIENT_494), Some(INGREDIENT_494), Some(INGREDIENT_494)];
const PATTERN_463: &[Option<Ingredient>] = &[Some(INGREDIENT_498), Some(INGREDIENT_498), Some(INGREDIENT_498), Some(INGREDIENT_498), Some(INGREDIENT_498), Some(INGREDIENT_498), Some(INGREDIENT_498), Some(INGREDIENT_498), Some(INGREDIENT_498)];
const PATTERN_464: &[Option<Ingredient>] = &[Some(INGREDIENT_70), Some(INGREDIENT_495), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_493), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70)];
const PATTERN_465: &[Option<Ingredient>] = &[Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_101), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_466: &[Option<Ingredient>] = &[Some(INGREDIENT_513), None, Some(INGREDIENT_513), Some(INGREDIENT_513), Some(INGREDIENT_513), Some(INGREDIENT_513)];
const PATTERN_467: &[Option<Ingredient>] = &[Some(INGREDIENT_513), Some(INGREDIENT_513), Some(INGREDIENT_513), Some(INGREDIENT_513), Some(INGREDIENT_513), Some(INGREDIENT_513)];
const PATTERN_468: &[Option<Ingredient>] = &[Some(INGREDIENT_513), Some(INGREDIENT_3), Some(INGREDIENT_513), Some(INGREDIENT_513), Some(INGREDIENT_3), Some(INGREDIENT_513)];
const PATTERN_469: &[Option<Ingredient>] = &[Some(INGREDIENT_3), Some(INGREDIENT_513), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_513), Some(INGREDIENT_3)];
const PATTERN_470: &[Option<Ingredient>] = &[Some(INGREDIENT_4), None, Some(INGREDIENT_4), Some(INGREDIENT_515), Some(INGREDIENT_515), Some(INGREDIENT_515), Some(INGREDIENT_515), Some(INGREDIENT_515), Some(INGREDIENT_515)];
const PATTERN_471: &[Option<Ingredient>] = &[Some(INGREDIENT_513), Some(INGREDIENT_513)];
const PATTERN_472: &[Option<Ingredient>] = &[Some(INGREDIENT_515), Some(INGREDIENT_515), Some(INGREDIENT_515), None, None, None, Some(INGREDIENT_515), Some(INGREDIENT_515), Some(INGREDIENT_515)];
const PATTERN_473: &[Option<Ingredient>] = &[Some(INGREDIENT_513), Some(INGREDIENT_513), Some(INGREDIENT_513), Some(INGREDIENT_513), Some(INGREDIENT_513), Some(INGREDIENT_513), None, Some(INGREDIENT_3), None];
const PATTERN_474: &[Option<Ingredient>] = &[Some(INGREDIENT_513), Some(INGREDIENT_513), Some(INGREDIENT_513)];
const PATTERN_475: &[Option<Ingredient>] = &[Some(INGREDIENT_513), None, None, Some(INGREDIENT_513), Some(INGREDIENT_513), None, Some(INGREDIENT_513), Some(INGREDIENT_513), Some(INGREDIENT_513)];
const PATTERN_476: &[Option<Ingredient>] = &[Some(INGREDIENT_517), Some(INGREDIENT_517), Some(INGREDIENT_517), Some(INGREDIENT_517)];
const PATTERN_477: &[Option<Ingredient>] = &[Some(INGREDIENT_12), Some(INGREDIENT_12), Some(INGREDIENT_12), Some(INGREDIENT_101), Some(INGREDIENT_101), Some(INGREDIENT_174), Some(INGREDIENT_12), Some(INGREDIENT_12), Some(INGREDIENT_12)];
const PATTERN_478: &[Option<Ingredient>] = &[Some(INGREDIENT_518), Some(INGREDIENT_518), Some(INGREDIENT_518), Some(INGREDIENT_518), Some(INGREDIENT_518), Some(INGREDIENT_518), None, Some(INGREDIENT_3), None];
const PATTERN_479: &[Option<Ingredient>] = &[Some(INGREDIENT_518), Some(INGREDIENT_518), Some(INGREDIENT_518), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_480: &[Option<Ingredient>] = &[Some(INGREDIENT_518), Some(INGREDIENT_518)];
const PATTERN_481: &[Option<Ingredient>] = &[Some(INGREDIENT_520), Some(INGREDIENT_520), Some(INGREDIENT_520)];
const PATTERN_482: &[Option<Ingredient>] = &[Some(INGREDIENT_520), None, None, Some(INGREDIENT_520), Some(INGREDIENT_520), None, Some(INGREDIENT_520), Some(INGREDIENT_520), Some(INGREDIENT_520)];
const PATTERN_483: &[Option<Ingredient>] = &[Some(INGREDIENT_521), Some(INGREDIENT_521), Some(INGREDIENT_521)];
const PATTERN_484: &[Option<Ingredient>] = &[Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_28), Some(INGREDIENT_518), Some(INGREDIENT_28)];
const PATTERN_485: &[Option<Ingredient>] = &[Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_313), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28)];
const PATTERN_486: &[Option<Ingredient>] = &[Some(INGREDIENT_526), Some(INGREDIENT_526), Some(INGREDIENT_526), Some(INGREDIENT_526), Some(INGREDIENT_526), Some(INGREDIENT_526)];
const PATTERN_487: &[Option<Ingredient>] = &[Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_313), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54)];
const PATTERN_488: &[Option<Ingredient>] = &[Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_313), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55)];
const PATTERN_489: &[Option<Ingredient>] = &[Some(INGREDIENT_518), Some(INGREDIENT_518), Some(INGREDIENT_518)];
const PATTERN_490: &[Option<Ingredient>] = &[Some(INGREDIENT_518), None, None, Some(INGREDIENT_518), Some(INGREDIENT_518), None, Some(INGREDIENT_518), Some(INGREDIENT_518), Some(INGREDIENT_518)];
const PATTERN_491: &[Option<Ingredient>] = &[Some(INGREDIENT_527), Some(INGREDIENT_527)];
const PATTERN_492: &[Option<Ingredient>] = &[None, Some(INGREDIENT_528), None, Some(INGREDIENT_528), Some(INGREDIENT_59), Some(INGREDIENT_528), None, Some(INGREDIENT_101), None];
const PATTERN_493: &[Option<Ingredient>] = &[None, Some(INGREDIENT_528), None, Some(INGREDIENT_528), None, Some(INGREDIENT_528), None, Some(INGREDIENT_528), None];
const PATTERN_494: &[Option<Ingredient>] = &[Some(INGREDIENT_528), Some(INGREDIENT_528), Some(INGREDIENT_528), Some(INGREDIENT_528)];
const PATTERN_495: &[Option<Ingredient>] = &[Some(INGREDIENT_529), Some(INGREDIENT_529), Some(INGREDIENT_529)];
const PATTERN_496: &[Option<Ingredient>] = &[Some(INGREDIENT_529), None, None, Some(INGREDIENT_529), Some(INGREDIENT_529), None, Some(INGREDIENT_529), Some(INGREDIENT_529), Some(INGREDIENT_529)];
const PATTERN_497: &[Option<Ingredient>] = &[Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_531), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_3)];
const PATTERN_498: &[Option<Ingredient>] = &[Some(INGREDIENT_532), Some(INGREDIENT_532)];
const PATTERN_499: &[Option<Ingredient>] = &[Some(INGREDIENT_533), None, Some(INGREDIENT_533), Some(INGREDIENT_533), Some(INGREDIENT_533), Some(INGREDIENT_533)];
const PATTERN_500: &[Option<Ingredient>] = &[Some(INGREDIENT_533), Some(INGREDIENT_533), Some(INGREDIENT_533), Some(INGREDIENT_533), Some(INGREDIENT_533), Some(INGREDIENT_533)];
const PATTERN_501: &[Option<Ingredient>] = &[Some(INGREDIENT_533), Some(INGREDIENT_3), Some(INGREDIENT_533), Some(INGREDIENT_533), Some(INGREDIENT_3), Some(INGREDIENT_533)];
const PATTERN_502: &[Option<Ingredient>] = &[Some(INGREDIENT_3), Some(INGREDIENT_533), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_533), Some(INGREDIENT_3)];
const PATTERN_503: &[Option<Ingredient>] = &[Some(INGREDIENT_4), None, Some(INGREDIENT_4), Some(INGREDIENT_535), Some(INGREDIENT_535), Some(INGREDIENT_535), Some(INGREDIENT_535), Some(INGREDIENT_535), Some(INGREDIENT_535)];
const PATTERN_504: &[Option<Ingredient>] = &[Some(INGREDIENT_533), Some(INGREDIENT_533)];
const PATTERN_505: &[Option<Ingredient>] = &[Some(INGREDIENT_535), Some(INGREDIENT_535), Some(INGREDIENT_535), None, None, None, Some(INGREDIENT_535), Some(INGREDIENT_535), Some(INGREDIENT_535)];
const PATTERN_506: &[Option<Ingredient>] = &[Some(INGREDIENT_533), Some(INGREDIENT_533), Some(INGREDIENT_533), Some(INGREDIENT_533), Some(INGREDIENT_533), Some(INGREDIENT_533), None, Some(INGREDIENT_3), None];
const PATTERN_507: &[Option<Ingredient>] = &[Some(INGREDIENT_533), Some(INGREDIENT_533), Some(INGREDIENT_533)];
const PATTERN_508: &[Option<Ingredient>] = &[Some(INGREDIENT_533), None, None, Some(INGREDIENT_533), Some(INGREDIENT_533), None, Some(INGREDIENT_533), Some(INGREDIENT_533), Some(INGREDIENT_533)];
const PATTERN_509: &[Option<Ingredient>] = &[Some(INGREDIENT_198), Some(INGREDIENT_198), Some(INGREDIENT_198), Some(INGREDIENT_198)];
const PATTERN_510: &[Option<Ingredient>] = &[Some(INGREDIENT_537), Some(INGREDIENT_537), Some(INGREDIENT_537)];
const PATTERN_511: &[Option<Ingredient>] = &[Some(INGREDIENT_538), Some(INGREDIENT_538), Some(INGREDIENT_538), Some(INGREDIENT_538), Some(INGREDIENT_538), Some(INGREDIENT_538), None, Some(INGREDIENT_3), None];
const PATTERN_512: &[Option<Ingredient>] = &[Some(INGREDIENT_538), Some(INGREDIENT_538), Some(INGREDIENT_538), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_513: &[Option<Ingredient>] = &[Some(INGREDIENT_538), Some(INGREDIENT_538)];
const PATTERN_514: &[Option<Ingredient>] = &[Some(INGREDIENT_540), Some(INGREDIENT_540), Some(INGREDIENT_540)];
const PATTERN_515: &[Option<Ingredient>] = &[Some(INGREDIENT_540), None, None, Some(INGREDIENT_540), Some(INGREDIENT_540), None, Some(INGREDIENT_540), Some(INGREDIENT_540), Some(INGREDIENT_540)];
const PATTERN_516: &[Option<Ingredient>] = &[Some(INGREDIENT_541), Some(INGREDIENT_541), Some(INGREDIENT_541)];
const PATTERN_517: &[Option<Ingredient>] = &[Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_28), Some(INGREDIENT_538), Some(INGREDIENT_28)];
const PATTERN_518: &[Option<Ingredient>] = &[Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_321), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28)];
const PATTERN_519: &[Option<Ingredient>] = &[Some(INGREDIENT_547), Some(INGREDIENT_547), Some(INGREDIENT_547), Some(INGREDIENT_547), Some(INGREDIENT_547), Some(INGREDIENT_547)];
const PATTERN_520: &[Option<Ingredient>] = &[Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_321), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54)];
const PATTERN_521: &[Option<Ingredient>] = &[Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_321), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55)];
const PATTERN_522: &[Option<Ingredient>] = &[Some(INGREDIENT_538), Some(INGREDIENT_538), Some(INGREDIENT_538)];
const PATTERN_523: &[Option<Ingredient>] = &[Some(INGREDIENT_538), None, None, Some(INGREDIENT_538), Some(INGREDIENT_538), None, Some(INGREDIENT_538), Some(INGREDIENT_538), Some(INGREDIENT_538)];
const PATTERN_524: &[Option<Ingredient>] = &[Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_12), Some(INGREDIENT_8), Some(INGREDIENT_12), Some(INGREDIENT_12), Some(INGREDIENT_101), Some(INGREDIENT_12)];
const PATTERN_525: &[Option<Ingredient>] = &[Some(INGREDIENT_13), Some(INGREDIENT_13), Some(INGREDIENT_13), Some(INGREDIENT_13)];
const PATTERN_526: &[Option<Ingredient>] = &[Some(INGREDIENT_548), Some(INGREDIENT_548), Some(INGREDIENT_548)];
const PATTERN_527: &[Option<Ingredient>] = &[Some(INGREDIENT_548), None, None, Some(INGREDIENT_548), Some(INGREDIENT_548), None, Some(INGREDIENT_548), Some(INGREDIENT_548), Some(INGREDIENT_548)];
const PATTERN_528: &[Option<Ingredient>] = &[Some(INGREDIENT_549), Some(INGREDIENT_549), Some(INGREDIENT_549), Some(INGREDIENT_549)];
const PATTERN_529: &[Option<Ingredient>] = &[Some(INGREDIENT_56), Some(INGREDIENT_56), Some(INGREDIENT_56), Some(INGREDIENT_56)];
const PATTERN_530: &[Option<Ingredient>] = &[Some(INGREDIENT_195), Some(INGREDIENT_195), Some(INGREDIENT_195)];
const PATTERN_531: &[Option<Ingredient>] = &[Some(INGREDIENT_195), None, None, Some(INGREDIENT_195), Some(INGREDIENT_195), None, Some(INGREDIENT_195), Some(INGREDIENT_195), Some(INGREDIENT_195)];
const PATTERN_532: &[Option<Ingredient>] = &[Some(INGREDIENT_195), Some(INGREDIENT_195), Some(INGREDIENT_195), Some(INGREDIENT_195), Some(INGREDIENT_195), Some(INGREDIENT_195)];
const PATTERN_533: &[Option<Ingredient>] = &[Some(INGREDIENT_147), Some(INGREDIENT_147), Some(INGREDIENT_147), Some(INGREDIENT_147)];
const PATTERN_534: &[Option<Ingredient>] = &[Some(INGREDIENT_147), Some(INGREDIENT_147)];
const PATTERN_535: &[Option<Ingredient>] = &[Some(INGREDIENT_147), Some(INGREDIENT_147), Some(INGREDIENT_147)];
const PATTERN_536: &[Option<Ingredient>] = &[Some(INGREDIENT_147), None, None, Some(INGREDIENT_147), Some(INGREDIENT_147), None, Some(INGREDIENT_147), Some(INGREDIENT_147), Some(INGREDIENT_147)];
const PATTERN_537: &[Option<Ingredient>] = &[Some(INGREDIENT_147), Some(INGREDIENT_147), Some(INGREDIENT_147), Some(INGREDIENT_147), Some(INGREDIENT_147), Some(INGREDIENT_147)];
const PATTERN_538: &[Option<Ingredient>] = &[Some(INGREDIENT_137), Some(INGREDIENT_137), Some(INGREDIENT_137), Some(INGREDIENT_137)];
const PATTERN_539: &[Option<Ingredient>] = &[Some(INGREDIENT_166), Some(INGREDIENT_166), Some(INGREDIENT_166)];
const PATTERN_540: &[Option<Ingredient>] = &[Some(INGREDIENT_166), None, None, Some(INGREDIENT_166), Some(INGREDIENT_166), None, Some(INGREDIENT_166), Some(INGREDIENT_166), Some(INGREDIENT_166)];
const PATTERN_541: &[Option<Ingredient>] = &[Some(INGREDIENT_166), Some(INGREDIENT_166), Some(INGREDIENT_166), Some(INGREDIENT_166), Some(INGREDIENT_166), Some(INGREDIENT_166)];
const PATTERN_542: &[Option<Ingredient>] = &[Some(INGREDIENT_142), Some(INGREDIENT_142), Some(INGREDIENT_142), Some(INGREDIENT_142)];
const PATTERN_543: &[Option<Ingredient>] = &[Some(INGREDIENT_225), Some(INGREDIENT_225), Some(INGREDIENT_225)];
const PATTERN_544: &[Option<Ingredient>] = &[Some(INGREDIENT_225), None, None, Some(INGREDIENT_225), Some(INGREDIENT_225), None, Some(INGREDIENT_225), Some(INGREDIENT_225), Some(INGREDIENT_225)];
const PATTERN_545: &[Option<Ingredient>] = &[Some(INGREDIENT_225), Some(INGREDIENT_225), Some(INGREDIENT_225), Some(INGREDIENT_225), Some(INGREDIENT_225), Some(INGREDIENT_225)];
const PATTERN_546: &[Option<Ingredient>] = &[Some(INGREDIENT_11), Some(INGREDIENT_11), Some(INGREDIENT_11), Some(INGREDIENT_11)];
const PATTERN_547: &[Option<Ingredient>] = &[Some(INGREDIENT_550), Some(INGREDIENT_550), Some(INGREDIENT_550)];
const PATTERN_548: &[Option<Ingredient>] = &[Some(INGREDIENT_550), None, None, Some(INGREDIENT_550), Some(INGREDIENT_550), None, Some(INGREDIENT_550), Some(INGREDIENT_550), Some(INGREDIENT_550)];
const PATTERN_549: &[Option<Ingredient>] = &[Some(INGREDIENT_395), Some(INGREDIENT_395), Some(INGREDIENT_395), Some(INGREDIENT_395)];
const PATTERN_550: &[Option<Ingredient>] = &[Some(INGREDIENT_551), Some(INGREDIENT_551), Some(INGREDIENT_551)];
const PATTERN_551: &[Option<Ingredient>] = &[Some(INGREDIENT_551), None, None, Some(INGREDIENT_551), Some(INGREDIENT_551), None, Some(INGREDIENT_551), Some(INGREDIENT_551), Some(INGREDIENT_551)];
const PATTERN_552: &[Option<Ingredient>] = &[Some(INGREDIENT_159), Some(INGREDIENT_159), Some(INGREDIENT_159), Some(INGREDIENT_159)];
const PATTERN_553: &[Option<Ingredient>] = &[Some(INGREDIENT_552), Some(INGREDIENT_552), Some(INGREDIENT_552)];
const PATTERN_554: &[Option<Ingredient>] = &[Some(INGREDIENT_552), None, None, Some(INGREDIENT_552), Some(INGREDIENT_552), None, Some(INGREDIENT_552), Some(INGREDIENT_552), Some(INGREDIENT_552)];
const PATTERN_555: &[Option<Ingredient>] = &[Some(INGREDIENT_552), Some(INGREDIENT_552), Some(INGREDIENT_552), Some(INGREDIENT_552), Some(INGREDIENT_552), Some(INGREDIENT_552)];
const PATTERN_556: &[Option<Ingredient>] = &[Some(INGREDIENT_164), Some(INGREDIENT_164), Some(INGREDIENT_164), Some(INGREDIENT_164)];
const PATTERN_557: &[Option<Ingredient>] = &[Some(INGREDIENT_162), Some(INGREDIENT_162), Some(INGREDIENT_162)];
const PATTERN_558: &[Option<Ingredient>] = &[Some(INGREDIENT_162), None, None, Some(INGREDIENT_162), Some(INGREDIENT_162), None, Some(INGREDIENT_162), Some(INGREDIENT_162), Some(INGREDIENT_162)];
const PATTERN_559: &[Option<Ingredient>] = &[Some(INGREDIENT_162), Some(INGREDIENT_162), Some(INGREDIENT_162), Some(INGREDIENT_162), Some(INGREDIENT_162), Some(INGREDIENT_162)];
const PATTERN_560: &[Option<Ingredient>] = &[Some(INGREDIENT_553), None, Some(INGREDIENT_553), Some(INGREDIENT_553), Some(INGREDIENT_553), Some(INGREDIENT_553)];
const PATTERN_561: &[Option<Ingredient>] = &[Some(INGREDIENT_553), Some(INGREDIENT_553), Some(INGREDIENT_553), Some(INGREDIENT_553), Some(INGREDIENT_553), Some(INGREDIENT_553)];
const PATTERN_562: &[Option<Ingredient>] = &[Some(INGREDIENT_553), Some(INGREDIENT_3), Some(INGREDIENT_553), Some(INGREDIENT_553), Some(INGREDIENT_3), Some(INGREDIENT_553)];
const PATTERN_563: &[Option<Ingredient>] = &[Some(INGREDIENT_3), Some(INGREDIENT_553), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_553), Some(INGREDIENT_3)];
const PATTERN_564: &[Option<Ingredient>] = &[Some(INGREDIENT_4), None, Some(INGREDIENT_4), Some(INGREDIENT_555), Some(INGREDIENT_555), Some(INGREDIENT_555), Some(INGREDIENT_555), Some(INGREDIENT_555), Some(INGREDIENT_555)];
const PATTERN_565: &[Option<Ingredient>] = &[Some(INGREDIENT_553), Some(INGREDIENT_553)];
const PATTERN_566: &[Option<Ingredient>] = &[Some(INGREDIENT_555), Some(INGREDIENT_555), Some(INGREDIENT_555), None, None, None, Some(INGREDIENT_555), Some(INGREDIENT_555), Some(INGREDIENT_555)];
const PATTERN_567: &[Option<Ingredient>] = &[Some(INGREDIENT_553), Some(INGREDIENT_553), Some(INGREDIENT_553), Some(INGREDIENT_553), Some(INGREDIENT_553), Some(INGREDIENT_553), None, Some(INGREDIENT_3), None];
const PATTERN_568: &[Option<Ingredient>] = &[Some(INGREDIENT_553), Some(INGREDIENT_553), Some(INGREDIENT_553)];
const PATTERN_569: &[Option<Ingredient>] = &[Some(INGREDIENT_553), None, None, Some(INGREDIENT_553), Some(INGREDIENT_553), None, Some(INGREDIENT_553), Some(INGREDIENT_553), Some(INGREDIENT_553)];
const PATTERN_570: &[Option<Ingredient>] = &[Some(INGREDIENT_557), Some(INGREDIENT_557), Some(INGREDIENT_557), Some(INGREDIENT_557)];
const PATTERN_571: &[Option<Ingredient>] = &[Some(INGREDIENT_167), None, Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_3), Some(INGREDIENT_167), Some(INGREDIENT_167), Some(INGREDIENT_101), Some(INGREDIENT_167)];
const PATTERN_572: &[Option<Ingredient>] = &[Some(INGREDIENT_222), Some(INGREDIENT_222), Some(INGREDIENT_222), Some(INGREDIENT_222)];
const PATTERN_573: &[Option<Ingredient>] = &[Some(INGREDIENT_559), Some(INGREDIENT_559), Some(INGREDIENT_559)];
const PATTERN_574: &[Option<Ingredient>] = &[Some(INGREDIENT_559), None, None, Some(INGREDIENT_559), Some(INGREDIENT_559), None, Some(INGREDIENT_559), Some(INGREDIENT_559), Some(INGREDIENT_559)];
const PATTERN_575: &[Option<Ingredient>] = &[Some(INGREDIENT_560), Some(INGREDIENT_560), Some(INGREDIENT_560)];
const PATTERN_576: &[Option<Ingredient>] = &[Some(INGREDIENT_560), None, None, Some(INGREDIENT_560), Some(INGREDIENT_560), None, Some(INGREDIENT_560), Some(INGREDIENT_560), Some(INGREDIENT_560)];
const PATTERN_577: &[Option<Ingredient>] = &[Some(INGREDIENT_560), Some(INGREDIENT_560), Some(INGREDIENT_560), Some(INGREDIENT_560), Some(INGREDIENT_560), Some(INGREDIENT_560)];
const PATTERN_578: &[Option<Ingredient>] = &[Some(INGREDIENT_562), Some(INGREDIENT_562), Some(INGREDIENT_562), Some(INGREDIENT_562), Some(INGREDIENT_562), Some(INGREDIENT_562), None, Some(INGREDIENT_3), None];
const PATTERN_579: &[Option<Ingredient>] = &[Some(INGREDIENT_562), Some(INGREDIENT_562), Some(INGREDIENT_562), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_580: &[Option<Ingredient>] = &[Some(INGREDIENT_562), Some(INGREDIENT_562)];
const PATTERN_581: &[Option<Ingredient>] = &[Some(INGREDIENT_564), Some(INGREDIENT_564), Some(INGREDIENT_564)];
const PATTERN_582: &[Option<Ingredient>] = &[Some(INGREDIENT_564), None, None, Some(INGREDIENT_564), Some(INGREDIENT_564), None, Some(INGREDIENT_564), Some(INGREDIENT_564), Some(INGREDIENT_564)];
const PATTERN_583: &[Option<Ingredient>] = &[Some(INGREDIENT_565), Some(INGREDIENT_565), Some(INGREDIENT_565)];
const PATTERN_584: &[Option<Ingredient>] = &[Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_28), Some(INGREDIENT_562), Some(INGREDIENT_28)];
const PATTERN_585: &[Option<Ingredient>] = &[Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_329), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28)];
const PATTERN_586: &[Option<Ingredient>] = &[Some(INGREDIENT_567), Some(INGREDIENT_567), Some(INGREDIENT_567), Some(INGREDIENT_567), Some(INGREDIENT_567), Some(INGREDIENT_567)];
const PATTERN_587: &[Option<Ingredient>] = &[Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_329), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54)];
const PATTERN_588: &[Option<Ingredient>] = &[Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_329), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55)];
const PATTERN_589: &[Option<Ingredient>] = &[Some(INGREDIENT_562), Some(INGREDIENT_562), Some(INGREDIENT_562)];
const PATTERN_590: &[Option<Ingredient>] = &[Some(INGREDIENT_562), None, None, Some(INGREDIENT_562), Some(INGREDIENT_562), None, Some(INGREDIENT_562), Some(INGREDIENT_562), Some(INGREDIENT_562)];
const PATTERN_591: &[Option<Ingredient>] = &[Some(INGREDIENT_366), Some(INGREDIENT_366), Some(INGREDIENT_366), Some(INGREDIENT_366)];
const PATTERN_592: &[Option<Ingredient>] = &[Some(INGREDIENT_568), Some(INGREDIENT_568)];
const PATTERN_593: &[Option<Ingredient>] = &[Some(INGREDIENT_570), Some(INGREDIENT_570), Some(INGREDIENT_570)];
const PATTERN_594: &[Option<Ingredient>] = &[Some(INGREDIENT_570), None, None, Some(INGREDIENT_570), Some(INGREDIENT_570), None, Some(INGREDIENT_570), Some(INGREDIENT_570), Some(INGREDIENT_570)];
const PATTERN_595: &[Option<Ingredient>] = &[Some(INGREDIENT_174), Some(INGREDIENT_174), Some(INGREDIENT_174), Some(INGREDIENT_174)];
const PATTERN_596: &[Option<Ingredient>] = &[Some(INGREDIENT_149), Some(INGREDIENT_149), Some(INGREDIENT_149), Some(INGREDIENT_149)];
const PATTERN_597: &[Option<Ingredient>] = &[Some(INGREDIENT_149), Some(INGREDIENT_149)];
const PATTERN_598: &[Option<Ingredient>] = &[Some(INGREDIENT_572), Some(INGREDIENT_572), Some(INGREDIENT_572)];
const PATTERN_599: &[Option<Ingredient>] = &[Some(INGREDIENT_572), None, None, Some(INGREDIENT_572), Some(INGREDIENT_572), None, Some(INGREDIENT_572), Some(INGREDIENT_572), Some(INGREDIENT_572)];
const PATTERN_600: &[Option<Ingredient>] = &[Some(INGREDIENT_8), None, Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_3), Some(INGREDIENT_8), Some(INGREDIENT_8), None, Some(INGREDIENT_8)];
const PATTERN_601: &[Option<Ingredient>] = &[Some(INGREDIENT_70), Some(INGREDIENT_577), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_55), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70)];
const PATTERN_602: &[Option<Ingredient>] = &[Some(INGREDIENT_188), Some(INGREDIENT_188), Some(INGREDIENT_188), Some(INGREDIENT_188), Some(INGREDIENT_188), Some(INGREDIENT_188), Some(INGREDIENT_188), Some(INGREDIENT_188), Some(INGREDIENT_188)];
const PATTERN_603: &[Option<Ingredient>] = &[Some(INGREDIENT_389), Some(INGREDIENT_389), Some(INGREDIENT_389), Some(INGREDIENT_389), Some(INGREDIENT_389), Some(INGREDIENT_389), Some(INGREDIENT_389), Some(INGREDIENT_389), Some(INGREDIENT_389)];
const PATTERN_604: &[Option<Ingredient>] = &[Some(INGREDIENT_420), Some(INGREDIENT_420), Some(INGREDIENT_420), Some(INGREDIENT_420), Some(INGREDIENT_420), Some(INGREDIENT_420), Some(INGREDIENT_420), Some(INGREDIENT_420), Some(INGREDIENT_420)];
const PATTERN_605: &[Option<Ingredient>] = &[Some(INGREDIENT_581), Some(INGREDIENT_581), Some(INGREDIENT_581), Some(INGREDIENT_581), Some(INGREDIENT_478), Some(INGREDIENT_581), Some(INGREDIENT_581), Some(INGREDIENT_581), Some(INGREDIENT_581)];
const PATTERN_606: &[Option<Ingredient>] = &[Some(INGREDIENT_582), Some(INGREDIENT_582), Some(INGREDIENT_582), Some(INGREDIENT_582), Some(INGREDIENT_582), Some(INGREDIENT_582), None, Some(INGREDIENT_3), None];
const PATTERN_607: &[Option<Ingredient>] = &[Some(INGREDIENT_582), Some(INGREDIENT_582), Some(INGREDIENT_582), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_608: &[Option<Ingredient>] = &[Some(INGREDIENT_582), Some(INGREDIENT_582)];
const PATTERN_609: &[Option<Ingredient>] = &[Some(INGREDIENT_584), Some(INGREDIENT_584), Some(INGREDIENT_584)];
const PATTERN_610: &[Option<Ingredient>] = &[Some(INGREDIENT_584), None, None, Some(INGREDIENT_584), Some(INGREDIENT_584), None, Some(INGREDIENT_584), Some(INGREDIENT_584), Some(INGREDIENT_584)];
const PATTERN_611: &[Option<Ingredient>] = &[Some(INGREDIENT_585), Some(INGREDIENT_585), Some(INGREDIENT_585)];
const PATTERN_612: &[Option<Ingredient>] = &[Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_28), Some(INGREDIENT_582), Some(INGREDIENT_28)];
const PATTERN_613: &[Option<Ingredient>] = &[Some(INGREDIENT_590), Some(INGREDIENT_590), Some(INGREDIENT_590)];
const PATTERN_614: &[Option<Ingredient>] = &[Some(INGREDIENT_590), None, None, Some(INGREDIENT_590), Some(INGREDIENT_590), None, Some(INGREDIENT_590), Some(INGREDIENT_590), Some(INGREDIENT_590)];
const PATTERN_615: &[Option<Ingredient>] = &[Some(INGREDIENT_590), Some(INGREDIENT_590), Some(INGREDIENT_590), Some(INGREDIENT_590), Some(INGREDIENT_590), Some(INGREDIENT_590)];
const PATTERN_616: &[Option<Ingredient>] = &[Some(INGREDIENT_494), Some(INGREDIENT_104), Some(INGREDIENT_104), Some(INGREDIENT_494)];
const PATTERN_617: &[Option<Ingredient>] = &[Some(INGREDIENT_591), Some(INGREDIENT_591), Some(INGREDIENT_591), Some(INGREDIENT_591)];
const PATTERN_618: &[Option<Ingredient>] = &[Some(INGREDIENT_592), Some(INGREDIENT_592), Some(INGREDIENT_592)];
const PATTERN_619: &[Option<Ingredient>] = &[Some(INGREDIENT_593), None, None, Some(INGREDIENT_593), Some(INGREDIENT_593), None, Some(INGREDIENT_593), Some(INGREDIENT_593), Some(INGREDIENT_593)];
const PATTERN_620: &[Option<Ingredient>] = &[Some(INGREDIENT_151), Some(INGREDIENT_151), Some(INGREDIENT_151), Some(INGREDIENT_151), Some(INGREDIENT_151), Some(INGREDIENT_151)];
const PATTERN_621: &[Option<Ingredient>] = &[Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_337), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28)];
const PATTERN_622: &[Option<Ingredient>] = &[Some(INGREDIENT_594), Some(INGREDIENT_594), Some(INGREDIENT_594), Some(INGREDIENT_594), Some(INGREDIENT_594), Some(INGREDIENT_594)];
const PATTERN_623: &[Option<Ingredient>] = &[Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_337), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54)];
const PATTERN_624: &[Option<Ingredient>] = &[Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_337), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55)];
const PATTERN_625: &[Option<Ingredient>] = &[Some(INGREDIENT_582), Some(INGREDIENT_582), Some(INGREDIENT_582)];
const PATTERN_626: &[Option<Ingredient>] = &[Some(INGREDIENT_582), None, None, Some(INGREDIENT_582), Some(INGREDIENT_582), None, Some(INGREDIENT_582), Some(INGREDIENT_582), Some(INGREDIENT_582)];
const PATTERN_627: &[Option<Ingredient>] = &[Some(INGREDIENT_101), Some(INGREDIENT_101), Some(INGREDIENT_101), Some(INGREDIENT_101), Some(INGREDIENT_101), Some(INGREDIENT_101), Some(INGREDIENT_101), Some(INGREDIENT_101), Some(INGREDIENT_101)];
const PATTERN_628: &[Option<Ingredient>] = &[None, Some(INGREDIENT_101), None, Some(INGREDIENT_101), Some(INGREDIENT_598), Some(INGREDIENT_101), None, Some(INGREDIENT_101), None];
const PATTERN_629: &[Option<Ingredient>] = &[Some(INGREDIENT_101), Some(INGREDIENT_3)];
const PATTERN_630: &[Option<Ingredient>] = &[Some(INGREDIENT_9), Some(INGREDIENT_101), Some(INGREDIENT_9), Some(INGREDIENT_98), Some(INGREDIENT_98), Some(INGREDIENT_98)];
const PATTERN_631: &[Option<Ingredient>] = &[Some(INGREDIENT_599), Some(INGREDIENT_599), Some(INGREDIENT_599), Some(INGREDIENT_599), Some(INGREDIENT_599), Some(INGREDIENT_599), Some(INGREDIENT_599), Some(INGREDIENT_599), Some(INGREDIENT_599)];
const PATTERN_632: &[Option<Ingredient>] = &[Some(INGREDIENT_153), Some(INGREDIENT_153), Some(INGREDIENT_153)];
const PATTERN_633: &[Option<Ingredient>] = &[Some(INGREDIENT_153), None, None, Some(INGREDIENT_153), Some(INGREDIENT_153), None, Some(INGREDIENT_153), Some(INGREDIENT_153), Some(INGREDIENT_153)];
const PATTERN_634: &[Option<Ingredient>] = &[Some(INGREDIENT_153), Some(INGREDIENT_153), Some(INGREDIENT_153), Some(INGREDIENT_153), Some(INGREDIENT_153), Some(INGREDIENT_153)];
const PATTERN_635: &[Option<Ingredient>] = &[Some(INGREDIENT_600), Some(INGREDIENT_600), Some(INGREDIENT_600), Some(INGREDIENT_600)];
const PATTERN_636: &[Option<Ingredient>] = &[Some(INGREDIENT_601), Some(INGREDIENT_601), Some(INGREDIENT_601), Some(INGREDIENT_598), Some(INGREDIENT_598), Some(INGREDIENT_598), Some(INGREDIENT_601), Some(INGREDIENT_601), Some(INGREDIENT_601)];
const PATTERN_637: &[Option<Ingredient>] = &[Some(INGREDIENT_70), Some(INGREDIENT_602), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_493), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70)];
const PATTERN_638: &[Option<Ingredient>] = &[None, Some(INGREDIENT_51), None, Some(INGREDIENT_51), Some(INGREDIENT_8), Some(INGREDIENT_51)];
const PATTERN_639: &[Option<Ingredient>] = &[Some(INGREDIENT_44), Some(INGREDIENT_44), Some(INGREDIENT_44), Some(INGREDIENT_44)];
const PATTERN_640: &[Option<Ingredient>] = &[Some(INGREDIENT_603), Some(INGREDIENT_603), Some(INGREDIENT_603)];
const PATTERN_641: &[Option<Ingredient>] = &[Some(INGREDIENT_604), None, None, Some(INGREDIENT_604), Some(INGREDIENT_604), None, Some(INGREDIENT_604), Some(INGREDIENT_604), Some(INGREDIENT_604)];
const PATTERN_642: &[Option<Ingredient>] = &[Some(INGREDIENT_155), Some(INGREDIENT_155), Some(INGREDIENT_155), Some(INGREDIENT_155), Some(INGREDIENT_155), Some(INGREDIENT_155)];
const PATTERN_643: &[Option<Ingredient>] = &[Some(INGREDIENT_19), Some(INGREDIENT_83), Some(INGREDIENT_19), Some(INGREDIENT_19), None, Some(INGREDIENT_19), Some(INGREDIENT_19), None, Some(INGREDIENT_19)];
const PATTERN_644: &[Option<Ingredient>] = &[Some(INGREDIENT_222), Some(INGREDIENT_605), Some(INGREDIENT_222), Some(INGREDIENT_605), Some(INGREDIENT_605), Some(INGREDIENT_605), Some(INGREDIENT_222), Some(INGREDIENT_605), Some(INGREDIENT_222)];
const PATTERN_645: &[Option<Ingredient>] = &[Some(INGREDIENT_70), Some(INGREDIENT_606), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_12), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70)];
const PATTERN_646: &[Option<Ingredient>] = &[Some(INGREDIENT_70), Some(INGREDIENT_607), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_55), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70)];
const PATTERN_647: &[Option<Ingredient>] = &[None, Some(INGREDIENT_8), Some(INGREDIENT_8), None];
const PATTERN_648: &[Option<Ingredient>] = &[Some(INGREDIENT_608), Some(INGREDIENT_8), Some(INGREDIENT_608), Some(INGREDIENT_608), Some(INGREDIENT_608), Some(INGREDIENT_608), None, Some(INGREDIENT_608), None];
const PATTERN_649: &[Option<Ingredient>] = &[Some(INGREDIENT_611), Some(INGREDIENT_1), Some(INGREDIENT_611)];
const PATTERN_650: &[Option<Ingredient>] = &[Some(INGREDIENT_70), Some(INGREDIENT_612), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_142), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70)];
const PATTERN_651: &[Option<Ingredient>] = &[Some(INGREDIENT_472), Some(INGREDIENT_472), Some(INGREDIENT_472), Some(INGREDIENT_472), Some(INGREDIENT_472), Some(INGREDIENT_472), Some(INGREDIENT_472), Some(INGREDIENT_472), Some(INGREDIENT_472)];
const PATTERN_652: &[Option<Ingredient>] = &[Some(INGREDIENT_8), Some(INGREDIENT_8), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_653: &[Option<Ingredient>] = &[None, Some(INGREDIENT_126), None, Some(INGREDIENT_126), Some(INGREDIENT_57), Some(INGREDIENT_126), None, Some(INGREDIENT_126), None];
const PATTERN_654: &[Option<Ingredient>] = &[Some(INGREDIENT_614), Some(INGREDIENT_614), Some(INGREDIENT_614)];
const PATTERN_655: &[Option<Ingredient>] = &[Some(INGREDIENT_614), None, None, Some(INGREDIENT_614), Some(INGREDIENT_614), None, Some(INGREDIENT_614), Some(INGREDIENT_614), Some(INGREDIENT_614)];
const PATTERN_656: &[Option<Ingredient>] = &[Some(INGREDIENT_615), Some(INGREDIENT_615), Some(INGREDIENT_615)];
const PATTERN_657: &[Option<Ingredient>] = &[Some(INGREDIENT_615), None, None, Some(INGREDIENT_615), Some(INGREDIENT_615), None, Some(INGREDIENT_615), Some(INGREDIENT_615), Some(INGREDIENT_615)];
const PATTERN_658: &[Option<Ingredient>] = &[Some(INGREDIENT_616), Some(INGREDIENT_616), Some(INGREDIENT_616)];
const PATTERN_659: &[Option<Ingredient>] = &[Some(INGREDIENT_616), None, None, Some(INGREDIENT_616), Some(INGREDIENT_616), None, Some(INGREDIENT_616), Some(INGREDIENT_616), Some(INGREDIENT_616)];
const PATTERN_660: &[Option<Ingredient>] = &[Some(INGREDIENT_58), Some(INGREDIENT_58), Some(INGREDIENT_58)];
const PATTERN_661: &[Option<Ingredient>] = &[Some(INGREDIENT_70), Some(INGREDIENT_617), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_56), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70)];
const PATTERN_662: &[Option<Ingredient>] = &[Some(INGREDIENT_618), Some(INGREDIENT_618), Some(INGREDIENT_618)];
const PATTERN_663: &[Option<Ingredient>] = &[Some(INGREDIENT_619), Some(INGREDIENT_619), Some(INGREDIENT_619), Some(INGREDIENT_619)];
const PATTERN_664: &[Option<Ingredient>] = &[None, Some(INGREDIENT_3), None, Some(INGREDIENT_3), Some(INGREDIENT_620), Some(INGREDIENT_3), Some(INGREDIENT_126), Some(INGREDIENT_126), Some(INGREDIENT_126)];
const PATTERN_665: &[Option<Ingredient>] = &[Some(INGREDIENT_417), Some(INGREDIENT_417), Some(INGREDIENT_417), Some(INGREDIENT_417), Some(INGREDIENT_621), Some(INGREDIENT_417), Some(INGREDIENT_417), Some(INGREDIENT_417), Some(INGREDIENT_417)];
const PATTERN_666: &[Option<Ingredient>] = &[Some(INGREDIENT_192), Some(INGREDIENT_3), Some(INGREDIENT_620)];
const PATTERN_667: &[Option<Ingredient>] = &[None, Some(INGREDIENT_102), None, Some(INGREDIENT_102), Some(INGREDIENT_622), Some(INGREDIENT_102), None, Some(INGREDIENT_102), None];
const PATTERN_668: &[Option<Ingredient>] = &[Some(INGREDIENT_70), Some(INGREDIENT_623), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_569), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70)];
const PATTERN_669: &[Option<Ingredient>] = &[Some(INGREDIENT_625), None, Some(INGREDIENT_625), Some(INGREDIENT_625), Some(INGREDIENT_625), Some(INGREDIENT_625)];
const PATTERN_670: &[Option<Ingredient>] = &[Some(INGREDIENT_625), Some(INGREDIENT_625), Some(INGREDIENT_625), Some(INGREDIENT_625), Some(INGREDIENT_625), Some(INGREDIENT_625)];
const PATTERN_671: &[Option<Ingredient>] = &[Some(INGREDIENT_625), Some(INGREDIENT_3), Some(INGREDIENT_625), Some(INGREDIENT_625), Some(INGREDIENT_3), Some(INGREDIENT_625)];
const PATTERN_672: &[Option<Ingredient>] = &[Some(INGREDIENT_3), Some(INGREDIENT_625), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_625), Some(INGREDIENT_3)];
const PATTERN_673: &[Option<Ingredient>] = &[Some(INGREDIENT_4), None, Some(INGREDIENT_4), Some(INGREDIENT_627), Some(INGREDIENT_627), Some(INGREDIENT_627), Some(INGREDIENT_627), Some(INGREDIENT_627), Some(INGREDIENT_627)];
const PATTERN_674: &[Option<Ingredient>] = &[Some(INGREDIENT_625), Some(INGREDIENT_625)];
const PATTERN_675: &[Option<Ingredient>] = &[Some(INGREDIENT_627), Some(INGREDIENT_627), Some(INGREDIENT_627), None, None, None, Some(INGREDIENT_627), Some(INGREDIENT_627), Some(INGREDIENT_627)];
const PATTERN_676: &[Option<Ingredient>] = &[Some(INGREDIENT_625), Some(INGREDIENT_625), Some(INGREDIENT_625), Some(INGREDIENT_625), Some(INGREDIENT_625), Some(INGREDIENT_625), None, Some(INGREDIENT_3), None];
const PATTERN_677: &[Option<Ingredient>] = &[Some(INGREDIENT_625), Some(INGREDIENT_625), Some(INGREDIENT_625)];
const PATTERN_678: &[Option<Ingredient>] = &[Some(INGREDIENT_625), None, None, Some(INGREDIENT_625), Some(INGREDIENT_625), None, Some(INGREDIENT_625), Some(INGREDIENT_625), Some(INGREDIENT_625)];
const PATTERN_679: &[Option<Ingredient>] = &[Some(INGREDIENT_629), Some(INGREDIENT_629), Some(INGREDIENT_629), Some(INGREDIENT_629)];
const PATTERN_680: &[Option<Ingredient>] = &[Some(INGREDIENT_10), Some(INGREDIENT_121), Some(INGREDIENT_121)];
const PATTERN_681: &[Option<Ingredient>] = &[Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_682: &[Option<Ingredient>] = &[Some(INGREDIENT_19), Some(INGREDIENT_19)];
const PATTERN_683: &[Option<Ingredient>] = &[Some(INGREDIENT_472), Some(INGREDIENT_630)];
const PATTERN_684: &[Option<Ingredient>] = &[Some(INGREDIENT_631), Some(INGREDIENT_631), Some(INGREDIENT_631), Some(INGREDIENT_3), None, Some(INGREDIENT_3)];
const PATTERN_685: &[Option<Ingredient>] = &[Some(INGREDIENT_157), Some(INGREDIENT_157), Some(INGREDIENT_157)];
const PATTERN_686: &[Option<Ingredient>] = &[Some(INGREDIENT_157), None, None, Some(INGREDIENT_157), Some(INGREDIENT_157), None, Some(INGREDIENT_157), Some(INGREDIENT_157), Some(INGREDIENT_157)];
const PATTERN_687: &[Option<Ingredient>] = &[Some(INGREDIENT_157), Some(INGREDIENT_157), Some(INGREDIENT_157), Some(INGREDIENT_157), Some(INGREDIENT_157), Some(INGREDIENT_157)];
const PATTERN_688: &[Option<Ingredient>] = &[Some(INGREDIENT_98), Some(INGREDIENT_98), Some(INGREDIENT_98), Some(INGREDIENT_98)];
const PATTERN_689: &[Option<Ingredient>] = &[Some(INGREDIENT_631), Some(INGREDIENT_631), None, Some(INGREDIENT_3), None, Some(INGREDIENT_3)];
const PATTERN_690: &[Option<Ingredient>] = &[Some(INGREDIENT_631), Some(INGREDIENT_631), Some(INGREDIENT_631), None, Some(INGREDIENT_3), None, None, Some(INGREDIENT_3), None];
const PATTERN_691: &[Option<Ingredient>] = &[Some(INGREDIENT_98), Some(INGREDIENT_98)];
const PATTERN_692: &[Option<Ingredient>] = &[Some(INGREDIENT_631), Some(INGREDIENT_3), Some(INGREDIENT_3)];
const PATTERN_693: &[Option<Ingredient>] = &[Some(INGREDIENT_98), Some(INGREDIENT_98), Some(INGREDIENT_98)];
const PATTERN_694: &[Option<Ingredient>] = &[None, None, Some(INGREDIENT_631), None, Some(INGREDIENT_3), None, Some(INGREDIENT_3), None, None];
const PATTERN_695: &[Option<Ingredient>] = &[Some(INGREDIENT_98), None, None, Some(INGREDIENT_98), Some(INGREDIENT_98), None, Some(INGREDIENT_98), Some(INGREDIENT_98), Some(INGREDIENT_98)];
const PATTERN_696: &[Option<Ingredient>] = &[Some(INGREDIENT_631), Some(INGREDIENT_631), Some(INGREDIENT_3)];
const PATTERN_697: &[Option<Ingredient>] = &[None, Some(INGREDIENT_8), None, Some(INGREDIENT_98), Some(INGREDIENT_98), Some(INGREDIENT_98)];
const PATTERN_698: &[Option<Ingredient>] = &[Some(INGREDIENT_632), Some(INGREDIENT_632), Some(INGREDIENT_632)];
const PATTERN_699: &[Option<Ingredient>] = &[Some(INGREDIENT_5), Some(INGREDIENT_5), Some(INGREDIENT_5), Some(INGREDIENT_5)];
const PATTERN_700: &[Option<Ingredient>] = &[Some(INGREDIENT_36), Some(INGREDIENT_36), Some(INGREDIENT_36), Some(INGREDIENT_36)];
const PATTERN_701: &[Option<Ingredient>] = &[Some(INGREDIENT_132), Some(INGREDIENT_132), Some(INGREDIENT_132), Some(INGREDIENT_132)];
const PATTERN_702: &[Option<Ingredient>] = &[Some(INGREDIENT_202), Some(INGREDIENT_202), Some(INGREDIENT_202), Some(INGREDIENT_202)];
const PATTERN_703: &[Option<Ingredient>] = &[Some(INGREDIENT_219), Some(INGREDIENT_219), Some(INGREDIENT_219), Some(INGREDIENT_219)];
const PATTERN_704: &[Option<Ingredient>] = &[Some(INGREDIENT_426), Some(INGREDIENT_426), Some(INGREDIENT_426), Some(INGREDIENT_426)];
const PATTERN_705: &[Option<Ingredient>] = &[Some(INGREDIENT_475), Some(INGREDIENT_475), Some(INGREDIENT_475), Some(INGREDIENT_475)];
const PATTERN_706: &[Option<Ingredient>] = &[Some(INGREDIENT_515), Some(INGREDIENT_515), Some(INGREDIENT_515), Some(INGREDIENT_515)];
const PATTERN_707: &[Option<Ingredient>] = &[Some(INGREDIENT_535), Some(INGREDIENT_535), Some(INGREDIENT_535), Some(INGREDIENT_535)];
const PATTERN_708: &[Option<Ingredient>] = &[Some(INGREDIENT_555), Some(INGREDIENT_555), Some(INGREDIENT_555), Some(INGREDIENT_555)];
const PATTERN_709: &[Option<Ingredient>] = &[Some(INGREDIENT_627), Some(INGREDIENT_627), Some(INGREDIENT_627), Some(INGREDIENT_627)];
const PATTERN_710: &[Option<Ingredient>] = &[Some(INGREDIENT_633), Some(INGREDIENT_633), Some(INGREDIENT_633), Some(INGREDIENT_633)];
const PATTERN_711: &[Option<Ingredient>] = &[Some(INGREDIENT_634), Some(INGREDIENT_634), Some(INGREDIENT_634)];
const PATTERN_712: &[Option<Ingredient>] = &[Some(INGREDIENT_634), None, None, Some(INGREDIENT_634), Some(INGREDIENT_634), None, Some(INGREDIENT_634), Some(INGREDIENT_634), Some(INGREDIENT_634)];
const PATTERN_713: &[Option<Ingredient>] = &[Some(INGREDIENT_634), Some(INGREDIENT_634), Some(INGREDIENT_634), Some(INGREDIENT_634), Some(INGREDIENT_634), Some(INGREDIENT_634)];
const PATTERN_714: &[Option<Ingredient>] = &[Some(INGREDIENT_552), Some(INGREDIENT_552), Some(INGREDIENT_552), Some(INGREDIENT_552)];
const PATTERN_715: &[Option<Ingredient>] = &[Some(INGREDIENT_635), Some(INGREDIENT_635), Some(INGREDIENT_635), Some(INGREDIENT_635)];
const PATTERN_716: &[Option<Ingredient>] = &[Some(INGREDIENT_159), Some(INGREDIENT_159), Some(INGREDIENT_159)];
const PATTERN_717: &[Option<Ingredient>] = &[Some(INGREDIENT_159), None, None, Some(INGREDIENT_159), Some(INGREDIENT_159), None, Some(INGREDIENT_159), Some(INGREDIENT_159), Some(INGREDIENT_159)];
const PATTERN_718: &[Option<Ingredient>] = &[Some(INGREDIENT_159), Some(INGREDIENT_159), Some(INGREDIENT_159), Some(INGREDIENT_159), Some(INGREDIENT_159), Some(INGREDIENT_159)];
const PATTERN_719: &[Option<Ingredient>] = &[None, Some(INGREDIENT_101), None, Some(INGREDIENT_101), Some(INGREDIENT_632), Some(INGREDIENT_101), None, Some(INGREDIENT_101), None];
const PATTERN_720: &[Option<Ingredient>] = &[Some(INGREDIENT_70), Some(INGREDIENT_639), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_560), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70)];
const PATTERN_721: &[Option<Ingredient>] = &[None, Some(INGREDIENT_10), None, Some(INGREDIENT_10), Some(INGREDIENT_28), Some(INGREDIENT_10), None, Some(INGREDIENT_10), None];
const PATTERN_722: &[Option<Ingredient>] = &[Some(INGREDIENT_106), Some(INGREDIENT_640), Some(INGREDIENT_106), Some(INGREDIENT_640), Some(INGREDIENT_106), Some(INGREDIENT_640), Some(INGREDIENT_106), Some(INGREDIENT_640), Some(INGREDIENT_106)];
const PATTERN_723: &[Option<Ingredient>] = &[Some(INGREDIENT_192), Some(INGREDIENT_3)];
const PATTERN_724: &[Option<Ingredient>] = &[Some(INGREDIENT_8), Some(INGREDIENT_3), Some(INGREDIENT_26)];
const PATTERN_725: &[Option<Ingredient>] = &[Some(INGREDIENT_163), Some(INGREDIENT_163), Some(INGREDIENT_163)];
const PATTERN_726: &[Option<Ingredient>] = &[Some(INGREDIENT_163), None, None, Some(INGREDIENT_163), Some(INGREDIENT_163), None, Some(INGREDIENT_163), Some(INGREDIENT_163), Some(INGREDIENT_163)];
const PATTERN_727: &[Option<Ingredient>] = &[Some(INGREDIENT_163), Some(INGREDIENT_163), Some(INGREDIENT_163), Some(INGREDIENT_163), Some(INGREDIENT_163), Some(INGREDIENT_163)];
const PATTERN_728: &[Option<Ingredient>] = &[Some(INGREDIENT_162), Some(INGREDIENT_162), Some(INGREDIENT_162), Some(INGREDIENT_162)];
const PATTERN_729: &[Option<Ingredient>] = &[Some(INGREDIENT_164), Some(INGREDIENT_164), Some(INGREDIENT_164)];
const PATTERN_730: &[Option<Ingredient>] = &[Some(INGREDIENT_164), None, None, Some(INGREDIENT_164), Some(INGREDIENT_164), None, Some(INGREDIENT_164), Some(INGREDIENT_164), Some(INGREDIENT_164)];
const PATTERN_731: &[Option<Ingredient>] = &[Some(INGREDIENT_164), Some(INGREDIENT_164), Some(INGREDIENT_164), Some(INGREDIENT_164), Some(INGREDIENT_164), Some(INGREDIENT_164)];
const PATTERN_732: &[Option<Ingredient>] = &[Some(INGREDIENT_642), Some(INGREDIENT_642), Some(INGREDIENT_642), Some(INGREDIENT_642), None, Some(INGREDIENT_642)];
const PATTERN_733: &[Option<Ingredient>] = &[Some(INGREDIENT_70), Some(INGREDIENT_643), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_12), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70)];
const PATTERN_734: &[Option<Ingredient>] = &[Some(INGREDIENT_70), Some(INGREDIENT_644), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_142), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70)];
const PATTERN_735: &[Option<Ingredient>] = &[Some(INGREDIENT_645), Some(INGREDIENT_645), Some(INGREDIENT_645), Some(INGREDIENT_645), Some(INGREDIENT_645), Some(INGREDIENT_645)];
const PATTERN_736: &[Option<Ingredient>] = &[Some(INGREDIENT_645), Some(INGREDIENT_3), Some(INGREDIENT_645), Some(INGREDIENT_645), Some(INGREDIENT_3), Some(INGREDIENT_645)];
const PATTERN_737: &[Option<Ingredient>] = &[Some(INGREDIENT_3), Some(INGREDIENT_645), Some(INGREDIENT_3), Some(INGREDIENT_3), Some(INGREDIENT_645), Some(INGREDIENT_3)];
const PATTERN_738: &[Option<Ingredient>] = &[Some(INGREDIENT_127), None, None, Some(INGREDIENT_646)];
const PATTERN_739: &[Option<Ingredient>] = &[Some(INGREDIENT_4), None, Some(INGREDIENT_4), Some(INGREDIENT_633), Some(INGREDIENT_633), Some(INGREDIENT_633), Some(INGREDIENT_633), Some(INGREDIENT_633), Some(INGREDIENT_633)];
const PATTERN_740: &[Option<Ingredient>] = &[Some(INGREDIENT_647), Some(INGREDIENT_647), Some(INGREDIENT_647), Some(INGREDIENT_647)];
const PATTERN_741: &[Option<Ingredient>] = &[Some(INGREDIENT_645), Some(INGREDIENT_645)];
const PATTERN_742: &[Option<Ingredient>] = &[Some(INGREDIENT_633), Some(INGREDIENT_633), Some(INGREDIENT_633), None, None, None, Some(INGREDIENT_633), Some(INGREDIENT_633), Some(INGREDIENT_633)];
const PATTERN_743: &[Option<Ingredient>] = &[Some(INGREDIENT_645), Some(INGREDIENT_645), Some(INGREDIENT_645), Some(INGREDIENT_645), Some(INGREDIENT_645), Some(INGREDIENT_645), None, Some(INGREDIENT_3), None];
const PATTERN_744: &[Option<Ingredient>] = &[Some(INGREDIENT_645), Some(INGREDIENT_645), Some(INGREDIENT_645)];
const PATTERN_745: &[Option<Ingredient>] = &[Some(INGREDIENT_645), None, None, Some(INGREDIENT_645), Some(INGREDIENT_645), None, Some(INGREDIENT_645), Some(INGREDIENT_645), Some(INGREDIENT_645)];
const PATTERN_746: &[Option<Ingredient>] = &[Some(INGREDIENT_649), Some(INGREDIENT_649)];
const PATTERN_747: &[Option<Ingredient>] = &[None, Some(INGREDIENT_189), None, Some(INGREDIENT_189), Some(INGREDIENT_59), Some(INGREDIENT_189), None, Some(INGREDIENT_101), None];
const PATTERN_748: &[Option<Ingredient>] = &[None, Some(INGREDIENT_189), None, Some(INGREDIENT_189), None, Some(INGREDIENT_189), None, Some(INGREDIENT_189), None];
const PATTERN_749: &[Option<Ingredient>] = &[Some(INGREDIENT_189), Some(INGREDIENT_189), Some(INGREDIENT_189), Some(INGREDIENT_189)];
const PATTERN_750: &[Option<Ingredient>] = &[Some(INGREDIENT_651), Some(INGREDIENT_651), Some(INGREDIENT_651)];
const PATTERN_751: &[Option<Ingredient>] = &[Some(INGREDIENT_651), None, None, Some(INGREDIENT_651), Some(INGREDIENT_651), None, Some(INGREDIENT_651), Some(INGREDIENT_651), Some(INGREDIENT_651)];
const PATTERN_752: &[Option<Ingredient>] = &[Some(INGREDIENT_662), Some(INGREDIENT_662)];
const PATTERN_753: &[Option<Ingredient>] = &[None, Some(INGREDIENT_664), None, Some(INGREDIENT_664), Some(INGREDIENT_59), Some(INGREDIENT_664), None, Some(INGREDIENT_101), None];
const PATTERN_754: &[Option<Ingredient>] = &[None, Some(INGREDIENT_664), None, Some(INGREDIENT_664), None, Some(INGREDIENT_664), None, Some(INGREDIENT_664), None];
const PATTERN_755: &[Option<Ingredient>] = &[Some(INGREDIENT_664), Some(INGREDIENT_664), Some(INGREDIENT_664), Some(INGREDIENT_664)];
const PATTERN_756: &[Option<Ingredient>] = &[Some(INGREDIENT_665), Some(INGREDIENT_665), Some(INGREDIENT_665)];
const PATTERN_757: &[Option<Ingredient>] = &[Some(INGREDIENT_665), None, None, Some(INGREDIENT_665), Some(INGREDIENT_665), None, Some(INGREDIENT_665), Some(INGREDIENT_665), Some(INGREDIENT_665)];
const PATTERN_758: &[Option<Ingredient>] = &[Some(INGREDIENT_678), Some(INGREDIENT_678)];
const PATTERN_759: &[Option<Ingredient>] = &[None, Some(INGREDIENT_680), None, Some(INGREDIENT_680), Some(INGREDIENT_59), Some(INGREDIENT_680), None, Some(INGREDIENT_101), None];
const PATTERN_760: &[Option<Ingredient>] = &[None, Some(INGREDIENT_680), None, Some(INGREDIENT_680), None, Some(INGREDIENT_680), None, Some(INGREDIENT_680), None];
const PATTERN_761: &[Option<Ingredient>] = &[Some(INGREDIENT_680), Some(INGREDIENT_680), Some(INGREDIENT_680), Some(INGREDIENT_680)];
const PATTERN_762: &[Option<Ingredient>] = &[Some(INGREDIENT_681), Some(INGREDIENT_681), Some(INGREDIENT_681)];
const PATTERN_763: &[Option<Ingredient>] = &[Some(INGREDIENT_681), None, None, Some(INGREDIENT_681), Some(INGREDIENT_681), None, Some(INGREDIENT_681), Some(INGREDIENT_681), Some(INGREDIENT_681)];
const PATTERN_764: &[Option<Ingredient>] = &[Some(INGREDIENT_693), Some(INGREDIENT_693)];
const PATTERN_765: &[Option<Ingredient>] = &[None, Some(INGREDIENT_695), None, Some(INGREDIENT_695), Some(INGREDIENT_59), Some(INGREDIENT_695), None, Some(INGREDIENT_101), None];
const PATTERN_766: &[Option<Ingredient>] = &[None, Some(INGREDIENT_695), None, Some(INGREDIENT_695), None, Some(INGREDIENT_695), None, Some(INGREDIENT_695), None];
const PATTERN_767: &[Option<Ingredient>] = &[Some(INGREDIENT_695), Some(INGREDIENT_695), Some(INGREDIENT_695), Some(INGREDIENT_695)];
const PATTERN_768: &[Option<Ingredient>] = &[Some(INGREDIENT_696), Some(INGREDIENT_696), Some(INGREDIENT_696)];
const PATTERN_769: &[Option<Ingredient>] = &[Some(INGREDIENT_696), None, None, Some(INGREDIENT_696), Some(INGREDIENT_696), None, Some(INGREDIENT_696), Some(INGREDIENT_696), Some(INGREDIENT_696)];
const PATTERN_770: &[Option<Ingredient>] = &[Some(INGREDIENT_70), Some(INGREDIENT_711), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_55), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70)];
const PATTERN_771: &[Option<Ingredient>] = &[Some(INGREDIENT_708), Some(INGREDIENT_708)];
const PATTERN_772: &[Option<Ingredient>] = &[None, Some(INGREDIENT_702), None, Some(INGREDIENT_702), Some(INGREDIENT_59), Some(INGREDIENT_702), None, Some(INGREDIENT_101), None];
const PATTERN_773: &[Option<Ingredient>] = &[None, Some(INGREDIENT_702), None, Some(INGREDIENT_702), None, Some(INGREDIENT_702), None, Some(INGREDIENT_702), None];
const PATTERN_774: &[Option<Ingredient>] = &[Some(INGREDIENT_702), Some(INGREDIENT_702), Some(INGREDIENT_702), Some(INGREDIENT_702)];
const PATTERN_775: &[Option<Ingredient>] = &[Some(INGREDIENT_707), Some(INGREDIENT_707), Some(INGREDIENT_707)];
const PATTERN_776: &[Option<Ingredient>] = &[Some(INGREDIENT_707), None, None, Some(INGREDIENT_707), Some(INGREDIENT_707), None, Some(INGREDIENT_707), Some(INGREDIENT_707), Some(INGREDIENT_707)];
const PATTERN_777: &[Option<Ingredient>] = &[Some(INGREDIENT_712), Some(INGREDIENT_712), Some(INGREDIENT_712), Some(INGREDIENT_712), Some(INGREDIENT_712), Some(INGREDIENT_712), None, Some(INGREDIENT_3), None];
const PATTERN_778: &[Option<Ingredient>] = &[Some(INGREDIENT_712), Some(INGREDIENT_712), Some(INGREDIENT_712), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_779: &[Option<Ingredient>] = &[Some(INGREDIENT_712), Some(INGREDIENT_712)];
const PATTERN_780: &[Option<Ingredient>] = &[Some(INGREDIENT_714), Some(INGREDIENT_714), Some(INGREDIENT_714)];
const PATTERN_781: &[Option<Ingredient>] = &[Some(INGREDIENT_714), None, None, Some(INGREDIENT_714), Some(INGREDIENT_714), None, Some(INGREDIENT_714), Some(INGREDIENT_714), Some(INGREDIENT_714)];
const PATTERN_782: &[Option<Ingredient>] = &[Some(INGREDIENT_715), Some(INGREDIENT_715), Some(INGREDIENT_715)];
const PATTERN_783: &[Option<Ingredient>] = &[Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_28), Some(INGREDIENT_712), Some(INGREDIENT_28)];
const PATTERN_784: &[Option<Ingredient>] = &[Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_345), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28)];
const PATTERN_785: &[Option<Ingredient>] = &[Some(INGREDIENT_717), Some(INGREDIENT_717), Some(INGREDIENT_717), Some(INGREDIENT_717), Some(INGREDIENT_717), Some(INGREDIENT_717)];
const PATTERN_786: &[Option<Ingredient>] = &[Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_345), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54)];
const PATTERN_787: &[Option<Ingredient>] = &[Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_345), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55)];
const PATTERN_788: &[Option<Ingredient>] = &[Some(INGREDIENT_83), Some(INGREDIENT_83), Some(INGREDIENT_83), Some(INGREDIENT_83)];
const PATTERN_789: &[Option<Ingredient>] = &[Some(INGREDIENT_712), Some(INGREDIENT_712), Some(INGREDIENT_712)];
const PATTERN_790: &[Option<Ingredient>] = &[Some(INGREDIENT_712), None, None, Some(INGREDIENT_712), Some(INGREDIENT_712), None, Some(INGREDIENT_712), Some(INGREDIENT_712), Some(INGREDIENT_712)];
const PATTERN_791: &[Option<Ingredient>] = &[Some(INGREDIENT_70), Some(INGREDIENT_718), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_484), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70), Some(INGREDIENT_70)];
const PATTERN_792: &[Option<Ingredient>] = &[Some(INGREDIENT_719), None, None, Some(INGREDIENT_719), Some(INGREDIENT_719), Some(INGREDIENT_719), Some(INGREDIENT_719), None, Some(INGREDIENT_719)];
const PATTERN_793: &[Option<Ingredient>] = &[Some(INGREDIENT_608), Some(INGREDIENT_608), Some(INGREDIENT_608), Some(INGREDIENT_3), None, Some(INGREDIENT_3)];
const PATTERN_794: &[Option<Ingredient>] = &[Some(INGREDIENT_608), Some(INGREDIENT_608), None, Some(INGREDIENT_3), None, Some(INGREDIENT_3)];
const PATTERN_795: &[Option<Ingredient>] = &[Some(INGREDIENT_608), Some(INGREDIENT_608), Some(INGREDIENT_608), None, Some(INGREDIENT_3), None, None, Some(INGREDIENT_3), None];
const PATTERN_796: &[Option<Ingredient>] = &[Some(INGREDIENT_608), Some(INGREDIENT_3), Some(INGREDIENT_3)];
const PATTERN_797: &[Option<Ingredient>] = &[None, None, Some(INGREDIENT_608), None, Some(INGREDIENT_3), None, Some(INGREDIENT_3), None, None];
const PATTERN_798: &[Option<Ingredient>] = &[Some(INGREDIENT_608), Some(INGREDIENT_608), Some(INGREDIENT_3)];
const PATTERN_799: &[Option<Ingredient>] = &[Some(INGREDIENT_721), Some(INGREDIENT_721), Some(INGREDIENT_721), Some(INGREDIENT_721), Some(INGREDIENT_721), Some(INGREDIENT_721), None, Some(INGREDIENT_3), None];
const PATTERN_800: &[Option<Ingredient>] = &[Some(INGREDIENT_721), Some(INGREDIENT_721), Some(INGREDIENT_721), Some(INGREDIENT_26), Some(INGREDIENT_26), Some(INGREDIENT_26)];
const PATTERN_801: &[Option<Ingredient>] = &[Some(INGREDIENT_721), Some(INGREDIENT_721)];
const PATTERN_802: &[Option<Ingredient>] = &[Some(INGREDIENT_723), Some(INGREDIENT_723), Some(INGREDIENT_723)];
const PATTERN_803: &[Option<Ingredient>] = &[Some(INGREDIENT_723), None, None, Some(INGREDIENT_723), Some(INGREDIENT_723), None, Some(INGREDIENT_723), Some(INGREDIENT_723), Some(INGREDIENT_723)];
const PATTERN_804: &[Option<Ingredient>] = &[Some(INGREDIENT_724), Some(INGREDIENT_724), Some(INGREDIENT_724)];
const PATTERN_805: &[Option<Ingredient>] = &[Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_51), Some(INGREDIENT_28), Some(INGREDIENT_721), Some(INGREDIENT_28)];
const PATTERN_806: &[Option<Ingredient>] = &[Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_353), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28), Some(INGREDIENT_28)];
const PATTERN_807: &[Option<Ingredient>] = &[Some(INGREDIENT_728), Some(INGREDIENT_728), Some(INGREDIENT_728), Some(INGREDIENT_728), Some(INGREDIENT_728), Some(INGREDIENT_728)];
const PATTERN_808: &[Option<Ingredient>] = &[Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_353), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54), Some(INGREDIENT_54)];
const PATTERN_809: &[Option<Ingredient>] = &[Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_353), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55), Some(INGREDIENT_55)];
const PATTERN_810: &[Option<Ingredient>] = &[Some(INGREDIENT_721), Some(INGREDIENT_721), Some(INGREDIENT_721)];
const PATTERN_811: &[Option<Ingredient>] = &[Some(INGREDIENT_721), None, None, Some(INGREDIENT_721), Some(INGREDIENT_721), None, Some(INGREDIENT_721), Some(INGREDIENT_721), Some(INGREDIENT_721)];
const INGREDIENTS_0: &[Ingredient] = &[INGREDIENT_0];
const INGREDIENTS_1: &[Ingredient] = &[INGREDIENT_1, INGREDIENT_2];
const INGREDIENTS_2: &[Ingredient] = &[INGREDIENT_6];
const INGREDIENTS_3: &[Ingredient] = &[INGREDIENT_11, INGREDIENT_12];
const INGREDIENTS_4: &[Ingredient] = &[INGREDIENT_19, INGREDIENT_19, INGREDIENT_19, INGREDIENT_19, INGREDIENT_19, INGREDIENT_19, INGREDIENT_19, INGREDIENT_19, INGREDIENT_19];
const INGREDIENTS_5: &[Ingredient] = &[INGREDIENT_20];
const INGREDIENTS_6: &[Ingredient] = &[INGREDIENT_1, INGREDIENT_21];
const INGREDIENTS_7: &[Ingredient] = &[INGREDIENT_25];
const INGREDIENTS_8: &[Ingredient] = &[INGREDIENT_32, INGREDIENT_33, INGREDIENT_33, INGREDIENT_33, INGREDIENT_33, INGREDIENT_33, INGREDIENT_33];
const INGREDIENTS_9: &[Ingredient] = &[INGREDIENT_34];
const INGREDIENTS_10: &[Ingredient] = &[INGREDIENT_1, INGREDIENT_35];
const INGREDIENTS_11: &[Ingredient] = &[INGREDIENT_37];
const INGREDIENTS_12: &[Ingredient] = &[INGREDIENT_43, INGREDIENT_42];
const INGREDIENTS_13: &[Ingredient] = &[INGREDIENT_42, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45];
const INGREDIENTS_14: &[Ingredient] = &[INGREDIENT_48];
const INGREDIENTS_15: &[Ingredient] = &[INGREDIENT_49];
const INGREDIENTS_16: &[Ingredient] = &[INGREDIENT_59];
const INGREDIENTS_17: &[Ingredient] = &[INGREDIENT_43, INGREDIENT_62];
const INGREDIENTS_18: &[Ingredient] = &[INGREDIENT_62, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45];
const INGREDIENTS_19: &[Ingredient] = &[INGREDIENT_65];
const INGREDIENTS_20: &[Ingredient] = &[INGREDIENT_66];
const INGREDIENTS_21: &[Ingredient] = &[INGREDIENT_68, INGREDIENT_68, INGREDIENT_68, INGREDIENT_68, INGREDIENT_68, INGREDIENT_68, INGREDIENT_68, INGREDIENT_68, INGREDIENT_68];
const INGREDIENTS_22: &[Ingredient] = &[INGREDIENT_76];
const INGREDIENTS_23: &[Ingredient] = &[INGREDIENT_77];
const INGREDIENTS_24: &[Ingredient] = &[INGREDIENT_78, INGREDIENT_78, INGREDIENT_78, INGREDIENT_51];
const INGREDIENTS_25: &[Ingredient] = &[INGREDIENT_78, INGREDIENT_82];
const INGREDIENTS_26: &[Ingredient] = &[INGREDIENT_43, INGREDIENT_115];
const INGREDIENTS_27: &[Ingredient] = &[INGREDIENT_115, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45];
const INGREDIENTS_28: &[Ingredient] = &[INGREDIENT_118];
const INGREDIENTS_29: &[Ingredient] = &[INGREDIENT_130];
const INGREDIENTS_30: &[Ingredient] = &[INGREDIENT_1, INGREDIENT_131];
const INGREDIENTS_31: &[Ingredient] = &[INGREDIENT_133];
const INGREDIENTS_32: &[Ingredient] = &[INGREDIENT_1, INGREDIENT_135];
const INGREDIENTS_33: &[Ingredient] = &[INGREDIENT_168];
const INGREDIENTS_34: &[Ingredient] = &[INGREDIENT_139];
const INGREDIENTS_35: &[Ingredient] = &[INGREDIENT_189];
const INGREDIENTS_36: &[Ingredient] = &[INGREDIENT_121];
const INGREDIENTS_37: &[Ingredient] = &[INGREDIENT_78, INGREDIENT_200];
const INGREDIENTS_38: &[Ingredient] = &[INGREDIENT_201];
const INGREDIENTS_39: &[Ingredient] = &[INGREDIENT_204];
const INGREDIENTS_40: &[Ingredient] = &[INGREDIENT_43, INGREDIENT_210];
const INGREDIENTS_41: &[Ingredient] = &[INGREDIENT_210, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45];
const INGREDIENTS_42: &[Ingredient] = &[INGREDIENT_62, INGREDIENT_213];
const INGREDIENTS_43: &[Ingredient] = &[INGREDIENT_214];
const INGREDIENTS_44: &[Ingredient] = &[INGREDIENT_217];
const INGREDIENTS_45: &[Ingredient] = &[INGREDIENT_1, INGREDIENT_218];
const INGREDIENTS_46: &[Ingredient] = &[INGREDIENT_220];
const INGREDIENTS_47: &[Ingredient] = &[INGREDIENT_227];
const INGREDIENTS_48: &[Ingredient] = &[INGREDIENT_233];
const INGREDIENTS_49: &[Ingredient] = &[INGREDIENT_42, INGREDIENT_238];
const INGREDIENTS_50: &[Ingredient] = &[INGREDIENT_42, INGREDIENT_239];
const INGREDIENTS_51: &[Ingredient] = &[INGREDIENT_42, INGREDIENT_240];
const INGREDIENTS_52: &[Ingredient] = &[INGREDIENT_42, INGREDIENT_241];
const INGREDIENTS_53: &[Ingredient] = &[INGREDIENT_42, INGREDIENT_242];
const INGREDIENTS_54: &[Ingredient] = &[INGREDIENT_42, INGREDIENT_243];
const INGREDIENTS_55: &[Ingredient] = &[INGREDIENT_42, INGREDIENT_244];
const INGREDIENTS_56: &[Ingredient] = &[INGREDIENT_62, INGREDIENT_245];
const INGREDIENTS_57: &[Ingredient] = &[INGREDIENT_62, INGREDIENT_246];
const INGREDIENTS_58: &[Ingredient] = &[INGREDIENT_62, INGREDIENT_247];
const INGREDIENTS_59: &[Ingredient] = &[INGREDIENT_62, INGREDIENT_248];
const INGREDIENTS_60: &[Ingredient] = &[INGREDIENT_62, INGREDIENT_249];
const INGREDIENTS_61: &[Ingredient] = &[INGREDIENT_62, INGREDIENT_250];
const INGREDIENTS_62: &[Ingredient] = &[INGREDIENT_62, INGREDIENT_251];
const INGREDIENTS_63: &[Ingredient] = &[INGREDIENT_115, INGREDIENT_252];
const INGREDIENTS_64: &[Ingredient] = &[INGREDIENT_115, INGREDIENT_253];
const INGREDIENTS_65: &[Ingredient] = &[INGREDIENT_115, INGREDIENT_254];
const INGREDIENTS_66: &[Ingredient] = &[INGREDIENT_115, INGREDIENT_255];
const INGREDIENTS_67: &[Ingredient] = &[INGREDIENT_115, INGREDIENT_256];
const INGREDIENTS_68: &[Ingredient] = &[INGREDIENT_115, INGREDIENT_257];
const INGREDIENTS_69: &[Ingredient] = &[INGREDIENT_115, INGREDIENT_258];
const INGREDIENTS_70: &[Ingredient] = &[INGREDIENT_210, INGREDIENT_259];
const INGREDIENTS_71: &[Ingredient] = &[INGREDIENT_210, INGREDIENT_260];
const INGREDIENTS_72: &[Ingredient] = &[INGREDIENT_210, INGREDIENT_261];
const INGREDIENTS_73: &[Ingredient] = &[INGREDIENT_210, INGREDIENT_262];
const INGREDIENTS_74: &[Ingredient] = &[INGREDIENT_210, INGREDIENT_263];
const INGREDIENTS_75: &[Ingredient] = &[INGREDIENT_210, INGREDIENT_264];
const INGREDIENTS_76: &[Ingredient] = &[INGREDIENT_210, INGREDIENT_265];
const INGREDIENTS_77: &[Ingredient] = &[INGREDIENT_266, INGREDIENT_267];
const INGREDIENTS_78: &[Ingredient] = &[INGREDIENT_266, INGREDIENT_268];
const INGREDIENTS_79: &[Ingredient] = &[INGREDIENT_266, INGREDIENT_269];
const INGREDIENTS_80: &[Ingredient] = &[INGREDIENT_266, INGREDIENT_270];
const INGREDIENTS_81: &[Ingredient] = &[INGREDIENT_266, INGREDIENT_271];
const INGREDIENTS_82: &[Ingredient] = &[INGREDIENT_266, INGREDIENT_272];
const INGREDIENTS_83: &[Ingredient] = &[INGREDIENT_266, INGREDIENT_273];
const INGREDIENTS_84: &[Ingredient] = &[INGREDIENT_213, INGREDIENT_274];
const INGREDIENTS_85: &[Ingredient] = &[INGREDIENT_213, INGREDIENT_275];
const INGREDIENTS_86: &[Ingredient] = &[INGREDIENT_213, INGREDIENT_276];
const INGREDIENTS_87: &[Ingredient] = &[INGREDIENT_213, INGREDIENT_277];
const INGREDIENTS_88: &[Ingredient] = &[INGREDIENT_213, INGREDIENT_278];
const INGREDIENTS_89: &[Ingredient] = &[INGREDIENT_213, INGREDIENT_279];
const INGREDIENTS_90: &[Ingredient] = &[INGREDIENT_213, INGREDIENT_280];
const INGREDIENTS_91: &[Ingredient] = &[INGREDIENT_281, INGREDIENT_282];
const INGREDIENTS_92: &[Ingredient] = &[INGREDIENT_281, INGREDIENT_283];
const INGREDIENTS_93: &[Ingredient] = &[INGREDIENT_281, INGREDIENT_284];
const INGREDIENTS_94: &[Ingredient] = &[INGREDIENT_281, INGREDIENT_285];
const INGREDIENTS_95: &[Ingredient] = &[INGREDIENT_281, INGREDIENT_286];
const INGREDIENTS_96: &[Ingredient] = &[INGREDIENT_281, INGREDIENT_287];
const INGREDIENTS_97: &[Ingredient] = &[INGREDIENT_281, INGREDIENT_288];
const INGREDIENTS_98: &[Ingredient] = &[INGREDIENT_289, INGREDIENT_290];
const INGREDIENTS_99: &[Ingredient] = &[INGREDIENT_289, INGREDIENT_291];
const INGREDIENTS_100: &[Ingredient] = &[INGREDIENT_289, INGREDIENT_292];
const INGREDIENTS_101: &[Ingredient] = &[INGREDIENT_289, INGREDIENT_293];
const INGREDIENTS_102: &[Ingredient] = &[INGREDIENT_289, INGREDIENT_294];
const INGREDIENTS_103: &[Ingredient] = &[INGREDIENT_289, INGREDIENT_295];
const INGREDIENTS_104: &[Ingredient] = &[INGREDIENT_289, INGREDIENT_296];
const INGREDIENTS_105: &[Ingredient] = &[INGREDIENT_297, INGREDIENT_298];
const INGREDIENTS_106: &[Ingredient] = &[INGREDIENT_297, INGREDIENT_299];
const INGREDIENTS_107: &[Ingredient] = &[INGREDIENT_297, INGREDIENT_300];
const INGREDIENTS_108: &[Ingredient] = &[INGREDIENT_297, INGREDIENT_301];
const INGREDIENTS_109: &[Ingredient] = &[INGREDIENT_297, INGREDIENT_302];
const INGREDIENTS_110: &[Ingredient] = &[INGREDIENT_297, INGREDIENT_303];
const INGREDIENTS_111: &[Ingredient] = &[INGREDIENT_297, INGREDIENT_304];
const INGREDIENTS_112: &[Ingredient] = &[INGREDIENT_305, INGREDIENT_306];
const INGREDIENTS_113: &[Ingredient] = &[INGREDIENT_305, INGREDIENT_307];
const INGREDIENTS_114: &[Ingredient] = &[INGREDIENT_305, INGREDIENT_308];
const INGREDIENTS_115: &[Ingredient] = &[INGREDIENT_305, INGREDIENT_309];
const INGREDIENTS_116: &[Ingredient] = &[INGREDIENT_305, INGREDIENT_310];
const INGREDIENTS_117: &[Ingredient] = &[INGREDIENT_305, INGREDIENT_311];
const INGREDIENTS_118: &[Ingredient] = &[INGREDIENT_305, INGREDIENT_312];
const INGREDIENTS_119: &[Ingredient] = &[INGREDIENT_313, INGREDIENT_314];
const INGREDIENTS_120: &[Ingredient] = &[INGREDIENT_313, INGREDIENT_315];
const INGREDIENTS_121: &[Ingredient] = &[INGREDIENT_313, INGREDIENT_316];
const INGREDIENTS_122: &[Ingredient] = &[INGREDIENT_313, INGREDIENT_317];
const INGREDIENTS_123: &[Ingredient] = &[INGREDIENT_313, INGREDIENT_318];
const INGREDIENTS_124: &[Ingredient] = &[INGREDIENT_313, INGREDIENT_319];
const INGREDIENTS_125: &[Ingredient] = &[INGREDIENT_313, INGREDIENT_320];
const INGREDIENTS_126: &[Ingredient] = &[INGREDIENT_321, INGREDIENT_322];
const INGREDIENTS_127: &[Ingredient] = &[INGREDIENT_321, INGREDIENT_323];
const INGREDIENTS_128: &[Ingredient] = &[INGREDIENT_321, INGREDIENT_324];
const INGREDIENTS_129: &[Ingredient] = &[INGREDIENT_321, INGREDIENT_325];
const INGREDIENTS_130: &[Ingredient] = &[INGREDIENT_321, INGREDIENT_326];
const INGREDIENTS_131: &[Ingredient] = &[INGREDIENT_321, INGREDIENT_327];
const INGREDIENTS_132: &[Ingredient] = &[INGREDIENT_321, INGREDIENT_328];
const INGREDIENTS_133: &[Ingredient] = &[INGREDIENT_329, INGREDIENT_330];
const INGREDIENTS_134: &[Ingredient] = &[INGREDIENT_329, INGREDIENT_331];
const INGREDIENTS_135: &[Ingredient] = &[INGREDIENT_329, INGREDIENT_332];
const INGREDIENTS_136: &[Ingredient] = &[INGREDIENT_329, INGREDIENT_333];
const INGREDIENTS_137: &[Ingredient] = &[INGREDIENT_329, INGREDIENT_334];
const INGREDIENTS_138: &[Ingredient] = &[INGREDIENT_329, INGREDIENT_335];
const INGREDIENTS_139: &[Ingredient] = &[INGREDIENT_329, INGREDIENT_336];
const INGREDIENTS_140: &[Ingredient] = &[INGREDIENT_337, INGREDIENT_338];
const INGREDIENTS_141: &[Ingredient] = &[INGREDIENT_337, INGREDIENT_339];
const INGREDIENTS_142: &[Ingredient] = &[INGREDIENT_337, INGREDIENT_340];
const INGREDIENTS_143: &[Ingredient] = &[INGREDIENT_337, INGREDIENT_341];
const INGREDIENTS_144: &[Ingredient] = &[INGREDIENT_337, INGREDIENT_342];
const INGREDIENTS_145: &[Ingredient] = &[INGREDIENT_337, INGREDIENT_343];
const INGREDIENTS_146: &[Ingredient] = &[INGREDIENT_337, INGREDIENT_344];
const INGREDIENTS_147: &[Ingredient] = &[INGREDIENT_345, INGREDIENT_346];
const INGREDIENTS_148: &[Ingredient] = &[INGREDIENT_345, INGREDIENT_347];
const INGREDIENTS_149: &[Ingredient] = &[INGREDIENT_345, INGREDIENT_348];
const INGREDIENTS_150: &[Ingredient] = &[INGREDIENT_345, INGREDIENT_349];
const INGREDIENTS_151: &[Ingredient] = &[INGREDIENT_345, INGREDIENT_350];
const INGREDIENTS_152: &[Ingredient] = &[INGREDIENT_345, INGREDIENT_351];
const INGREDIENTS_153: &[Ingredient] = &[INGREDIENT_345, INGREDIENT_352];
const INGREDIENTS_154: &[Ingredient] = &[INGREDIENT_353, INGREDIENT_354];
const INGREDIENTS_155: &[Ingredient] = &[INGREDIENT_353, INGREDIENT_355];
const INGREDIENTS_156: &[Ingredient] = &[INGREDIENT_353, INGREDIENT_356];
const INGREDIENTS_157: &[Ingredient] = &[INGREDIENT_353, INGREDIENT_357];
const INGREDIENTS_158: &[Ingredient] = &[INGREDIENT_353, INGREDIENT_358];
const INGREDIENTS_159: &[Ingredient] = &[INGREDIENT_353, INGREDIENT_359];
const INGREDIENTS_160: &[Ingredient] = &[INGREDIENT_353, INGREDIENT_360];
const INGREDIENTS_161: &[Ingredient] = &[INGREDIENT_361];
const INGREDIENTS_162: &[Ingredient] = &[INGREDIENT_369, INGREDIENT_86];
const INGREDIENTS_163: &[Ingredient] = &[INGREDIENT_97, INGREDIENT_374, INGREDIENT_99];
const INGREDIENTS_164: &[Ingredient] = &[INGREDIENT_78, INGREDIENT_111];
const INGREDIENTS_165: &[Ingredient] = &[INGREDIENT_106, INGREDIENT_86, INGREDIENT_192];
const INGREDIENTS_166: &[Ingredient] = &[INGREDIENT_106, INGREDIENT_78];
const INGREDIENTS_167: &[Ingredient] = &[INGREDIENT_8, INGREDIENT_16];
const INGREDIENTS_168: &[Ingredient] = &[INGREDIENT_78, INGREDIENT_381];
const INGREDIENTS_169: &[Ingredient] = &[INGREDIENT_57, INGREDIENT_135];
const INGREDIENTS_170: &[Ingredient] = &[INGREDIENT_384, INGREDIENT_385];
const INGREDIENTS_171: &[Ingredient] = &[INGREDIENT_390];
const INGREDIENTS_172: &[Ingredient] = &[INGREDIENT_167];
const INGREDIENTS_173: &[Ingredient] = &[INGREDIENT_11, INGREDIENT_174];
const INGREDIENTS_174: &[Ingredient] = &[INGREDIENT_43, INGREDIENT_266];
const INGREDIENTS_175: &[Ingredient] = &[INGREDIENT_266, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45];
const INGREDIENTS_176: &[Ingredient] = &[INGREDIENT_42, INGREDIENT_345];
const INGREDIENTS_177: &[Ingredient] = &[INGREDIENT_400];
const INGREDIENTS_178: &[Ingredient] = &[INGREDIENT_43, INGREDIENT_213];
const INGREDIENTS_179: &[Ingredient] = &[INGREDIENT_213, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45];
const INGREDIENTS_180: &[Ingredient] = &[INGREDIENT_84, INGREDIENT_84, INGREDIENT_84, INGREDIENT_84, INGREDIENT_84, INGREDIENT_84, INGREDIENT_84, INGREDIENT_84, INGREDIENT_84];
const INGREDIENTS_181: &[Ingredient] = &[INGREDIENT_412, INGREDIENT_413, INGREDIENT_413, INGREDIENT_413, INGREDIENT_413];
const INGREDIENTS_182: &[Ingredient] = &[INGREDIENT_414, INGREDIENT_135];
const INGREDIENTS_183: &[Ingredient] = &[INGREDIENT_14];
const INGREDIENTS_184: &[Ingredient] = &[INGREDIENT_8];
const INGREDIENTS_185: &[Ingredient] = &[INGREDIENT_424];
const INGREDIENTS_186: &[Ingredient] = &[INGREDIENT_1, INGREDIENT_425];
const INGREDIENTS_187: &[Ingredient] = &[INGREDIENT_427];
const INGREDIENTS_188: &[Ingredient] = &[INGREDIENT_429];
const INGREDIENTS_189: &[Ingredient] = &[INGREDIENT_43, INGREDIENT_281];
const INGREDIENTS_190: &[Ingredient] = &[INGREDIENT_281, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45];
const INGREDIENTS_191: &[Ingredient] = &[INGREDIENT_444];
const INGREDIENTS_192: &[Ingredient] = &[INGREDIENT_62, INGREDIENT_345];
const INGREDIENTS_193: &[Ingredient] = &[INGREDIENT_43, INGREDIENT_289];
const INGREDIENTS_194: &[Ingredient] = &[INGREDIENT_289, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45];
const INGREDIENTS_195: &[Ingredient] = &[INGREDIENT_451];
const INGREDIENTS_196: &[Ingredient] = &[INGREDIENT_42, INGREDIENT_345, INGREDIENT_345];
const INGREDIENTS_197: &[Ingredient] = &[INGREDIENT_266, INGREDIENT_345];
const INGREDIENTS_198: &[Ingredient] = &[INGREDIENT_381];
const INGREDIENTS_199: &[Ingredient] = &[INGREDIENT_452];
const INGREDIENTS_200: &[Ingredient] = &[INGREDIENT_43, INGREDIENT_297];
const INGREDIENTS_201: &[Ingredient] = &[INGREDIENT_297, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45];
const INGREDIENTS_202: &[Ingredient] = &[INGREDIENT_213, INGREDIENT_345];
const INGREDIENTS_203: &[Ingredient] = &[INGREDIENT_43, INGREDIENT_305];
const INGREDIENTS_204: &[Ingredient] = &[INGREDIENT_305, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45];
const INGREDIENTS_205: &[Ingredient] = &[INGREDIENT_468];
const INGREDIENTS_206: &[Ingredient] = &[INGREDIENT_62, INGREDIENT_337, INGREDIENT_321];
const INGREDIENTS_207: &[Ingredient] = &[INGREDIENT_62, INGREDIENT_337, INGREDIENT_337, INGREDIENT_345];
const INGREDIENTS_208: &[Ingredient] = &[INGREDIENT_469];
const INGREDIENTS_209: &[Ingredient] = &[INGREDIENT_329, INGREDIENT_321];
const INGREDIENTS_210: &[Ingredient] = &[INGREDIENT_86, INGREDIENT_472];
const INGREDIENTS_211: &[Ingredient] = &[INGREDIENT_473];
const INGREDIENTS_212: &[Ingredient] = &[INGREDIENT_1, INGREDIENT_474];
const INGREDIENTS_213: &[Ingredient] = &[INGREDIENT_476];
const INGREDIENTS_214: &[Ingredient] = &[INGREDIENT_383, INGREDIENT_383, INGREDIENT_383, INGREDIENT_383, INGREDIENT_383, INGREDIENT_383, INGREDIENT_383, INGREDIENT_383, INGREDIENT_383];
const INGREDIENTS_215: &[Ingredient] = &[INGREDIENT_383];
const INGREDIENTS_216: &[Ingredient] = &[INGREDIENT_78, INGREDIENT_482];
const INGREDIENTS_217: &[Ingredient] = &[INGREDIENT_12, INGREDIENT_483];
const INGREDIENTS_218: &[Ingredient] = &[INGREDIENT_12, INGREDIENT_82];
const INGREDIENTS_219: &[Ingredient] = &[INGREDIENT_157, INGREDIENT_483];
const INGREDIENTS_220: &[Ingredient] = &[INGREDIENT_157, INGREDIENT_82];
const INGREDIENTS_221: &[Ingredient] = &[INGREDIENT_488, INGREDIENT_489];
const INGREDIENTS_222: &[Ingredient] = &[INGREDIENT_490, INGREDIENT_490, INGREDIENT_32];
const INGREDIENTS_223: &[Ingredient] = &[INGREDIENT_491, INGREDIENT_491, INGREDIENT_491, INGREDIENT_491, INGREDIENT_491, INGREDIENT_491, INGREDIENT_491, INGREDIENT_491, INGREDIENT_491];
const INGREDIENTS_224: &[Ingredient] = &[INGREDIENT_104, INGREDIENT_104, INGREDIENT_104, INGREDIENT_104, INGREDIENT_104, INGREDIENT_104, INGREDIENT_104, INGREDIENT_104, INGREDIENT_104];
const INGREDIENTS_225: &[Ingredient] = &[INGREDIENT_504, INGREDIENT_504, INGREDIENT_504, INGREDIENT_504, INGREDIENT_167, INGREDIENT_167, INGREDIENT_167, INGREDIENT_167];
const INGREDIENTS_226: &[Ingredient] = &[INGREDIENT_505];
const INGREDIENTS_227: &[Ingredient] = &[INGREDIENT_513];
const INGREDIENTS_228: &[Ingredient] = &[INGREDIENT_1, INGREDIENT_514];
const INGREDIENTS_229: &[Ingredient] = &[INGREDIENT_516];
const INGREDIENTS_230: &[Ingredient] = &[INGREDIENT_43, INGREDIENT_313];
const INGREDIENTS_231: &[Ingredient] = &[INGREDIENT_313, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45];
const INGREDIENTS_232: &[Ingredient] = &[INGREDIENT_522];
const INGREDIENTS_233: &[Ingredient] = &[INGREDIENT_523];
const INGREDIENTS_234: &[Ingredient] = &[INGREDIENT_337, INGREDIENT_353];
const INGREDIENTS_235: &[Ingredient] = &[INGREDIENT_524];
const INGREDIENTS_236: &[Ingredient] = &[INGREDIENT_530, INGREDIENT_530, INGREDIENT_530, INGREDIENT_530, INGREDIENT_530, INGREDIENT_530, INGREDIENT_530, INGREDIENT_530, INGREDIENT_530];
const INGREDIENTS_237: &[Ingredient] = &[INGREDIENT_488, INGREDIENT_84];
const INGREDIENTS_238: &[Ingredient] = &[INGREDIENT_533];
const INGREDIENTS_239: &[Ingredient] = &[INGREDIENT_1, INGREDIENT_534];
const INGREDIENTS_240: &[Ingredient] = &[INGREDIENT_536];
const INGREDIENTS_241: &[Ingredient] = &[INGREDIENT_43, INGREDIENT_321];
const INGREDIENTS_242: &[Ingredient] = &[INGREDIENT_321, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45];
const INGREDIENTS_243: &[Ingredient] = &[INGREDIENT_542];
const INGREDIENTS_244: &[Ingredient] = &[INGREDIENT_543];
const INGREDIENTS_245: &[Ingredient] = &[INGREDIENT_544];
const INGREDIENTS_246: &[Ingredient] = &[INGREDIENT_545];
const INGREDIENTS_247: &[Ingredient] = &[INGREDIENT_337, INGREDIENT_345];
const INGREDIENTS_248: &[Ingredient] = &[INGREDIENT_147];
const INGREDIENTS_249: &[Ingredient] = &[INGREDIENT_553];
const INGREDIENTS_250: &[Ingredient] = &[INGREDIENT_1, INGREDIENT_554];
const INGREDIENTS_251: &[Ingredient] = &[INGREDIENT_556];
const INGREDIENTS_252: &[Ingredient] = &[INGREDIENT_159, INGREDIENT_159, INGREDIENT_159, INGREDIENT_159, INGREDIENT_159, INGREDIENT_159, INGREDIENT_159, INGREDIENT_159, INGREDIENT_159];
const INGREDIENTS_253: &[Ingredient] = &[INGREDIENT_222, INGREDIENT_222, INGREDIENT_222, INGREDIENT_222, INGREDIENT_222, INGREDIENT_222, INGREDIENT_222, INGREDIENT_222, INGREDIENT_222];
const INGREDIENTS_254: &[Ingredient] = &[INGREDIENT_561, INGREDIENT_99, INGREDIENT_123];
const INGREDIENTS_255: &[Ingredient] = &[INGREDIENT_561];
const INGREDIENTS_256: &[Ingredient] = &[INGREDIENT_43, INGREDIENT_329];
const INGREDIENTS_257: &[Ingredient] = &[INGREDIENT_329, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45];
const INGREDIENTS_258: &[Ingredient] = &[INGREDIENT_62, INGREDIENT_337];
const INGREDIENTS_259: &[Ingredient] = &[INGREDIENT_573, INGREDIENT_574, INGREDIENT_32, INGREDIENT_128, INGREDIENT_374];
const INGREDIENTS_260: &[Ingredient] = &[INGREDIENT_573, INGREDIENT_574, INGREDIENT_32, INGREDIENT_128, INGREDIENT_575];
const INGREDIENTS_261: &[Ingredient] = &[INGREDIENT_573, INGREDIENT_574, INGREDIENT_32, INGREDIENT_128, INGREDIENT_576];
const INGREDIENTS_262: &[Ingredient] = &[INGREDIENT_578];
const INGREDIENTS_263: &[Ingredient] = &[INGREDIENT_579];
const INGREDIENTS_264: &[Ingredient] = &[INGREDIENT_580];
const INGREDIENTS_265: &[Ingredient] = &[INGREDIENT_43, INGREDIENT_337];
const INGREDIENTS_266: &[Ingredient] = &[INGREDIENT_337, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45];
const INGREDIENTS_267: &[Ingredient] = &[INGREDIENT_33];
const INGREDIENTS_268: &[Ingredient] = &[INGREDIENT_586];
const INGREDIENTS_269: &[Ingredient] = &[INGREDIENT_587];
const INGREDIENTS_270: &[Ingredient] = &[INGREDIENT_588];
const INGREDIENTS_271: &[Ingredient] = &[INGREDIENT_595];
const INGREDIENTS_272: &[Ingredient] = &[INGREDIENT_199];
const INGREDIENTS_273: &[Ingredient] = &[INGREDIENT_78, INGREDIENT_613];
const INGREDIENTS_274: &[Ingredient] = &[INGREDIENT_96];
const INGREDIENTS_275: &[Ingredient] = &[INGREDIENT_625];
const INGREDIENTS_276: &[Ingredient] = &[INGREDIENT_1, INGREDIENT_626];
const INGREDIENTS_277: &[Ingredient] = &[INGREDIENT_628];
const INGREDIENTS_278: &[Ingredient] = &[INGREDIENT_98];
const INGREDIENTS_279: &[Ingredient] = &[INGREDIENT_411];
const INGREDIENTS_280: &[Ingredient] = &[INGREDIENT_537];
const INGREDIENTS_281: &[Ingredient] = &[INGREDIENT_32, INGREDIENT_490, INGREDIENT_490, INGREDIENT_468];
const INGREDIENTS_282: &[Ingredient] = &[INGREDIENT_32, INGREDIENT_490, INGREDIENT_490, INGREDIENT_451];
const INGREDIENTS_283: &[Ingredient] = &[INGREDIENT_32, INGREDIENT_490, INGREDIENT_490, INGREDIENT_444];
const INGREDIENTS_284: &[Ingredient] = &[INGREDIENT_32, INGREDIENT_490, INGREDIENT_490, INGREDIENT_400];
const INGREDIENTS_285: &[Ingredient] = &[INGREDIENT_32, INGREDIENT_490, INGREDIENT_490, INGREDIENT_66];
const INGREDIENTS_286: &[Ingredient] = &[INGREDIENT_32, INGREDIENT_490, INGREDIENT_490, INGREDIENT_394];
const INGREDIENTS_287: &[Ingredient] = &[INGREDIENT_32, INGREDIENT_490, INGREDIENT_490, INGREDIENT_636];
const INGREDIENTS_288: &[Ingredient] = &[INGREDIENT_32, INGREDIENT_490, INGREDIENT_490, INGREDIENT_637];
const INGREDIENTS_289: &[Ingredient] = &[INGREDIENT_32, INGREDIENT_490, INGREDIENT_490, INGREDIENT_522];
const INGREDIENTS_290: &[Ingredient] = &[INGREDIENT_32, INGREDIENT_490, INGREDIENT_490, INGREDIENT_523];
const INGREDIENTS_291: &[Ingredient] = &[INGREDIENT_32, INGREDIENT_490, INGREDIENT_490, INGREDIENT_381];
const INGREDIENTS_292: &[Ingredient] = &[INGREDIENT_32, INGREDIENT_490, INGREDIENT_490, INGREDIENT_545];
const INGREDIENTS_293: &[Ingredient] = &[INGREDIENT_32, INGREDIENT_490, INGREDIENT_490, INGREDIENT_586];
const INGREDIENTS_294: &[Ingredient] = &[INGREDIENT_32, INGREDIENT_490, INGREDIENT_490, INGREDIENT_588];
const INGREDIENTS_295: &[Ingredient] = &[INGREDIENT_32, INGREDIENT_490, INGREDIENT_490, INGREDIENT_524];
const INGREDIENTS_296: &[Ingredient] = &[INGREDIENT_32, INGREDIENT_490, INGREDIENT_490, INGREDIENT_452];
const INGREDIENTS_297: &[Ingredient] = &[INGREDIENT_32, INGREDIENT_490, INGREDIENT_490, INGREDIENT_49];
const INGREDIENTS_298: &[Ingredient] = &[INGREDIENT_641, INGREDIENT_135];
const INGREDIENTS_299: &[Ingredient] = &[INGREDIENT_1, INGREDIENT_205];
const INGREDIENTS_300: &[Ingredient] = &[INGREDIENT_645];
const INGREDIENTS_301: &[Ingredient] = &[INGREDIENT_648];
const INGREDIENTS_302: &[Ingredient] = &[INGREDIENT_650, INGREDIENT_31];
const INGREDIENTS_303: &[Ingredient] = &[INGREDIENT_652, INGREDIENT_31];
const INGREDIENTS_304: &[Ingredient] = &[INGREDIENT_139, INGREDIENT_31];
const INGREDIENTS_305: &[Ingredient] = &[INGREDIENT_653, INGREDIENT_31];
const INGREDIENTS_306: &[Ingredient] = &[INGREDIENT_654, INGREDIENT_31];
const INGREDIENTS_307: &[Ingredient] = &[INGREDIENT_655, INGREDIENT_31];
const INGREDIENTS_308: &[Ingredient] = &[INGREDIENT_656, INGREDIENT_31];
const INGREDIENTS_309: &[Ingredient] = &[INGREDIENT_657, INGREDIENT_31];
const INGREDIENTS_310: &[Ingredient] = &[INGREDIENT_658, INGREDIENT_31];
const INGREDIENTS_311: &[Ingredient] = &[INGREDIENT_659, INGREDIENT_31];
const INGREDIENTS_312: &[Ingredient] = &[INGREDIENT_660, INGREDIENT_31];
const INGREDIENTS_313: &[Ingredient] = &[INGREDIENT_140, INGREDIENT_31];
const INGREDIENTS_314: &[Ingredient] = &[INGREDIENT_138, INGREDIENT_31];
const INGREDIENTS_315: &[Ingredient] = &[INGREDIENT_661, INGREDIENT_31];
const INGREDIENTS_316: &[Ingredient] = &[INGREDIENT_663, INGREDIENT_31];
const INGREDIENTS_317: &[Ingredient] = &[INGREDIENT_666, INGREDIENT_31];
const INGREDIENTS_318: &[Ingredient] = &[INGREDIENT_667, INGREDIENT_31];
const INGREDIENTS_319: &[Ingredient] = &[INGREDIENT_668, INGREDIENT_31];
const INGREDIENTS_320: &[Ingredient] = &[INGREDIENT_669, INGREDIENT_31];
const INGREDIENTS_321: &[Ingredient] = &[INGREDIENT_670, INGREDIENT_31];
const INGREDIENTS_322: &[Ingredient] = &[INGREDIENT_371, INGREDIENT_31];
const INGREDIENTS_323: &[Ingredient] = &[INGREDIENT_671, INGREDIENT_31];
const INGREDIENTS_324: &[Ingredient] = &[INGREDIENT_672, INGREDIENT_31];
const INGREDIENTS_325: &[Ingredient] = &[INGREDIENT_673, INGREDIENT_31];
const INGREDIENTS_326: &[Ingredient] = &[INGREDIENT_674, INGREDIENT_31];
const INGREDIENTS_327: &[Ingredient] = &[INGREDIENT_372, INGREDIENT_31];
const INGREDIENTS_328: &[Ingredient] = &[INGREDIENT_370, INGREDIENT_31];
const INGREDIENTS_329: &[Ingredient] = &[INGREDIENT_675, INGREDIENT_31];
const INGREDIENTS_330: &[Ingredient] = &[INGREDIENT_676, INGREDIENT_31];
const INGREDIENTS_331: &[Ingredient] = &[INGREDIENT_677, INGREDIENT_31];
const INGREDIENTS_332: &[Ingredient] = &[INGREDIENT_679, INGREDIENT_31];
const INGREDIENTS_333: &[Ingredient] = &[INGREDIENT_682, INGREDIENT_31];
const INGREDIENTS_334: &[Ingredient] = &[INGREDIENT_683, INGREDIENT_31];
const INGREDIENTS_335: &[Ingredient] = &[INGREDIENT_684, INGREDIENT_31];
const INGREDIENTS_336: &[Ingredient] = &[INGREDIENT_685, INGREDIENT_31];
const INGREDIENTS_337: &[Ingredient] = &[INGREDIENT_686, INGREDIENT_31];
const INGREDIENTS_338: &[Ingredient] = &[INGREDIENT_528, INGREDIENT_31];
const INGREDIENTS_339: &[Ingredient] = &[INGREDIENT_687, INGREDIENT_31];
const INGREDIENTS_340: &[Ingredient] = &[INGREDIENT_688, INGREDIENT_31];
const INGREDIENTS_341: &[Ingredient] = &[INGREDIENT_689, INGREDIENT_31];
const INGREDIENTS_342: &[Ingredient] = &[INGREDIENT_690, INGREDIENT_31];
const INGREDIENTS_343: &[Ingredient] = &[INGREDIENT_529, INGREDIENT_31];
const INGREDIENTS_344: &[Ingredient] = &[INGREDIENT_527, INGREDIENT_31];
const INGREDIENTS_345: &[Ingredient] = &[INGREDIENT_691, INGREDIENT_31];
const INGREDIENTS_346: &[Ingredient] = &[INGREDIENT_692, INGREDIENT_31];
const INGREDIENTS_347: &[Ingredient] = &[INGREDIENT_694, INGREDIENT_31];
const INGREDIENTS_348: &[Ingredient] = &[INGREDIENT_697, INGREDIENT_31];
const INGREDIENTS_349: &[Ingredient] = &[INGREDIENT_698, INGREDIENT_31];
const INGREDIENTS_350: &[Ingredient] = &[INGREDIENT_699, INGREDIENT_31];
const INGREDIENTS_351: &[Ingredient] = &[INGREDIENT_700, INGREDIENT_31];
const INGREDIENTS_352: &[Ingredient] = &[INGREDIENT_701, INGREDIENT_31];
const INGREDIENTS_353: &[Ingredient] = &[INGREDIENT_702, INGREDIENT_31];
const INGREDIENTS_354: &[Ingredient] = &[INGREDIENT_703, INGREDIENT_31];
const INGREDIENTS_355: &[Ingredient] = &[INGREDIENT_704, INGREDIENT_31];
const INGREDIENTS_356: &[Ingredient] = &[INGREDIENT_705, INGREDIENT_31];
const INGREDIENTS_357: &[Ingredient] = &[INGREDIENT_706, INGREDIENT_31];
const INGREDIENTS_358: &[Ingredient] = &[INGREDIENT_707, INGREDIENT_31];
const INGREDIENTS_359: &[Ingredient] = &[INGREDIENT_708, INGREDIENT_31];
const INGREDIENTS_360: &[Ingredient] = &[INGREDIENT_709, INGREDIENT_31];
const INGREDIENTS_361: &[Ingredient] = &[INGREDIENT_710, INGREDIENT_31];
const INGREDIENTS_362: &[Ingredient] = &[INGREDIENT_632];
const INGREDIENTS_363: &[Ingredient] = &[INGREDIENT_43, INGREDIENT_345];
const INGREDIENTS_364: &[Ingredient] = &[INGREDIENT_345, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45];
const INGREDIENTS_365: &[Ingredient] = &[INGREDIENT_75];
const INGREDIENTS_366: &[Ingredient] = &[INGREDIENT_637];
const INGREDIENTS_367: &[Ingredient] = &[INGREDIENT_87];
const INGREDIENTS_368: &[Ingredient] = &[INGREDIENT_81, INGREDIENT_48, INGREDIENT_17];
const INGREDIENTS_369: &[Ingredient] = &[INGREDIENT_43, INGREDIENT_353];
const INGREDIENTS_370: &[Ingredient] = &[INGREDIENT_353, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_44, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45, INGREDIENT_45];
const INGREDIENTS_371: &[Ingredient] = &[INGREDIENT_394];
const INGREDIENTS_372: &[Ingredient] = &[INGREDIENT_636];
const INGREDIENTS_373: &[Ingredient] = &[INGREDIENT_725];
const INGREDIENTS_374: &[Ingredient] = &[INGREDIENT_726];
const SHAPES_0: &[(FireworkShape, Ingredient)] = &[(FireworkShape::Burst, INGREDIENT_17), (FireworkShape::Creeper, INGREDIENT_376), (FireworkShape::LargeBall, INGREDIENT_377), (FireworkShape::Star, INGREDIENT_378)];
/// 官方数据包的全部配方，顺序就是注册表顺序。
pub static RECIPES: &[Recipe] = &[
    Recipe { id: "minecraft:acacia_boat", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_0, result: Stack { item: 983, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:acacia_button", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_0, result: Stack { item: 862, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:acacia_chest_boat", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_1, result: Stack { item: 984, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:acacia_door", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_1, result: Stack { item: 893, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:acacia_fence", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_2, result: Stack { item: 421, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:acacia_fence_gate", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_3, result: Stack { item: 936, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:acacia_hanging_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_4, result: Stack { item: 1119, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:acacia_planks", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_2, result: Stack { item: 67, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:acacia_pressure_plate", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_5, result: Stack { item: 879, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:acacia_shelf", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_6, result: Stack { item: 377, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:acacia_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_7, result: Stack { item: 1106, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:acacia_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_8, result: Stack { item: 345, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:acacia_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_9, result: Stack { item: 519, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:acacia_trapdoor", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_1, result: Stack { item: 915, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:acacia_wood", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_10, result: Stack { item: 207, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:activator_rail", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_11, result: Stack { item: 948, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:amethyst_block", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_12, result: Stack { item: 117, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:andesite", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_3, result: Stack { item: 6, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:andesite_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_13, result: Stack { item: 815, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:andesite_slab_from_andesite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_13, result: Stack { item: 815, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:andesite_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_14, result: Stack { item: 798, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:andesite_stairs_from_andesite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_13, result: Stack { item: 798, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:andesite_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_15, result: Stack { item: 541, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:andesite_wall_from_andesite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_13, result: Stack { item: 541, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:anvil", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_16, result: Stack { item: 553, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:armor_stand", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_17, result: Stack { item: 1405, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:arrow", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_18, result: Stack { item: 1009, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:baked_potato", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_18, result: Stack { item: 1380, count: 1, components: &[0, 0] }, experience: 0.35, time: 200 } },
    Recipe { id: "minecraft:baked_potato_from_campfire_cooking", kind: RecipeKind::Cooking { kind: CookingKind::Campfire, ingredient: INGREDIENT_18, result: Stack { item: 1380, count: 1, components: &[0, 0] }, experience: 0.35, time: 600 } },
    Recipe { id: "minecraft:baked_potato_from_smoking", kind: RecipeKind::Cooking { kind: CookingKind::Smoking, ingredient: INGREDIENT_18, result: Stack { item: 1380, count: 1, components: &[0, 0] }, experience: 0.35, time: 200 } },
    Recipe { id: "minecraft:bamboo_block", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_4, result: Stack { item: 177, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:bamboo_button", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_5, result: Stack { item: 868, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:bamboo_chest_raft", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_6, result: Stack { item: 996, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:bamboo_door", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_19, result: Stack { item: 899, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:bamboo_fence", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_20, result: Stack { item: 427, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:bamboo_fence_gate", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_21, result: Stack { item: 942, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:bamboo_hanging_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_22, result: Stack { item: 1125, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:bamboo_mosaic", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_23, result: Stack { item: 76, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:bamboo_mosaic_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_24, result: Stack { item: 352, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:bamboo_mosaic_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_25, result: Stack { item: 526, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:bamboo_planks", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_7, result: Stack { item: 73, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:bamboo_pressure_plate", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_26, result: Stack { item: 885, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:bamboo_raft", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_27, result: Stack { item: 995, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:bamboo_shelf", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_28, result: Stack { item: 378, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:bamboo_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_29, result: Stack { item: 1112, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:bamboo_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_30, result: Stack { item: 351, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:bamboo_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_31, result: Stack { item: 525, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:bamboo_trapdoor", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_19, result: Stack { item: 921, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:barrel", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_32, result: Stack { item: 1506, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:beacon", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_33, result: Stack { item: 530, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:beehive", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_34, result: Stack { item: 1532, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:beetroot_soup", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_8, result: Stack { item: 1440, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:birch_boat", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_35, result: Stack { item: 979, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:birch_button", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_9, result: Stack { item: 860, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:birch_chest_boat", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_10, result: Stack { item: 980, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:birch_door", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_36, result: Stack { item: 891, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:birch_fence", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_37, result: Stack { item: 419, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:birch_fence_gate", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_38, result: Stack { item: 934, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:birch_hanging_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_39, result: Stack { item: 1117, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:birch_planks", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_11, result: Stack { item: 65, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:birch_pressure_plate", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_40, result: Stack { item: 877, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:birch_shelf", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_41, result: Stack { item: 379, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:birch_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_42, result: Stack { item: 1104, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:birch_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_43, result: Stack { item: 343, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:birch_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_44, result: Stack { item: 517, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:birch_trapdoor", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_36, result: Stack { item: 913, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:birch_wood", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_45, result: Stack { item: 205, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:black_banner", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_46, result: Stack { item: 1432, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:black_banner_duplicate", kind: RecipeKind::BannerDuplicate { banner: INGREDIENT_40, result: Stack { item: 1432, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:black_bed", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_47, result: Stack { item: 1234, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:black_bundle", kind: RecipeKind::Transmute { input: INGREDIENT_41, material: INGREDIENT_42, min_material: 1, max_material: 1, result_item: Some(1169), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:black_candle", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_12, result: Stack { item: 1566, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:black_carpet", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_48, result: Stack { item: 595, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:black_concrete_powder", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_13, result: Stack { item: 752, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:black_concrete_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_49, result: Stack { item: 736, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:black_concrete_slab_from_black_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_46, result: Stack { item: 736, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:black_concrete_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_50, result: Stack { item: 720, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:black_concrete_stairs_from_black_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_46, result: Stack { item: 720, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:black_cushion", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_51, result: Stack { item: 1185, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:black_dye", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_14, result: Stack { item: 1214, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:black_dye_from_wither_rose", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_15, result: Stack { item: 1214, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:black_glazed_terracotta", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_50, result: Stack { item: 688, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:black_harness", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_52, result: Stack { item: 965, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:black_shulker_box", kind: RecipeKind::Transmute { input: INGREDIENT_52, material: INGREDIENT_42, min_material: 1, max_material: 1, result_item: Some(672), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:black_stained_glass", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_53, result: Stack { item: 620, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:black_stained_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_54, result: Stack { item: 636, count: 16, components: &[0, 0] } } },
    Recipe { id: "minecraft:black_stained_glass_pane_from_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_55, result: Stack { item: 636, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:black_terracotta", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_56, result: Stack { item: 576, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:black_wool_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_57, result: Stack { item: 297, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:black_wool_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_58, result: Stack { item: 281, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:blackstone_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_59, result: Stack { item: 1538, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:blackstone_slab_from_blackstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_56, result: Stack { item: 1538, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:blackstone_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_60, result: Stack { item: 1539, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:blackstone_stairs_from_blackstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_56, result: Stack { item: 1539, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:blackstone_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_61, result: Stack { item: 546, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:blackstone_wall_from_blackstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_56, result: Stack { item: 546, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:blast_furnace", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_62, result: Stack { item: 1508, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:blaze_powder", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_16, result: Stack { item: 1274, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:blue_banner", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_63, result: Stack { item: 1428, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:blue_banner_duplicate", kind: RecipeKind::BannerDuplicate { banner: INGREDIENT_61, result: Stack { item: 1428, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:blue_bed", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_64, result: Stack { item: 1230, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:blue_bundle", kind: RecipeKind::Transmute { input: INGREDIENT_41, material: INGREDIENT_62, min_material: 1, max_material: 1, result_item: Some(1165), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:blue_candle", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_17, result: Stack { item: 1562, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:blue_carpet", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_65, result: Stack { item: 591, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:blue_concrete_powder", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_18, result: Stack { item: 748, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:blue_concrete_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_66, result: Stack { item: 732, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:blue_concrete_slab_from_blue_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_63, result: Stack { item: 732, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:blue_concrete_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_67, result: Stack { item: 716, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:blue_concrete_stairs_from_blue_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_63, result: Stack { item: 716, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:blue_cushion", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_68, result: Stack { item: 1181, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:blue_dye", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_19, result: Stack { item: 1210, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:blue_dye_from_cornflower", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_20, result: Stack { item: 1210, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:blue_glazed_terracotta", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_67, result: Stack { item: 684, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:blue_harness", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_69, result: Stack { item: 961, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:blue_ice", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_21, result: Stack { item: 786, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:blue_shulker_box", kind: RecipeKind::Transmute { input: INGREDIENT_52, material: INGREDIENT_62, min_material: 1, max_material: 1, result_item: Some(668), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:blue_stained_glass", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_70, result: Stack { item: 616, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:blue_stained_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_71, result: Stack { item: 632, count: 16, components: &[0, 0] } } },
    Recipe { id: "minecraft:blue_stained_glass_pane_from_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_72, result: Stack { item: 632, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:blue_terracotta", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_73, result: Stack { item: 572, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:blue_wool_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_74, result: Stack { item: 293, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:blue_wool_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_75, result: Stack { item: 277, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:bolt_armor_trim_smithing_template", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_76, result: Stack { item: 1597, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:bolt_armor_trim_smithing_template_smithing_trim", kind: RecipeKind::SmithingTrim { template: INGREDIENT_71, base: INGREDIENT_73, addition: INGREDIENT_74, pattern: "minecraft:bolt" } },
    Recipe { id: "minecraft:bone_block", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_77, result: Stack { item: 654, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:bone_meal", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_22, result: Stack { item: 1215, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:bone_meal_from_bone_block", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_23, result: Stack { item: 1215, count: 9, components: &[0, 0] } } },
    Recipe { id: "minecraft:book", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_24, result: Stack { item: 1146, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:book_cloning", kind: RecipeKind::BookCloning { source: INGREDIENT_79, material: INGREDIENT_80, min_generation: 0, max_generation: 1, result: Stack { item: 1372, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:bookshelf", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_78, result: Stack { item: 390, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:bordure_indented_banner_pattern", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_25, result: Stack { item: 1503, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:bow", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_79, result: Stack { item: 1008, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:bowl", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_80, result: Stack { item: 1006, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:bread", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_81, result: Stack { item: 1067, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_awkward_blaze_powder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_86, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 34, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_awkward_breeze_rod", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_87, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 42, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_awkward_cobweb", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_88, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 43, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_awkward_ghast_tear", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_89, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 31, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_awkward_glistering_melon_slice", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_90, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 24, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_awkward_golden_carrot", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_91, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 4, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_awkward_magma_cream", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_92, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 11, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_awkward_phantom_membrane", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_93, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 40, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_awkward_pufferfish", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_94, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 22, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_awkward_rabbit_foot", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_95, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 8, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_awkward_slime_block", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_96, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 44, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_awkward_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_97, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 28, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_awkward_stone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_98, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 45, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_awkward_sugar", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_99, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 13, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_awkward_turtle_helmet", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_100, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 19, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_fire_resistance_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[11]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 12, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_harming_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[26]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 27, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_healing_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[24]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 26, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_healing_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[24]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 25, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_invisibility_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[6]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 7, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_leaping_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[8]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 16, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_leaping_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[8]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 10, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_leaping_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[8]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 9, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_long_leaping_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[9]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 17, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_long_night_vision_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[5]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 7, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_long_poison_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[29]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 26, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_long_swiftness_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[14]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 17, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_night_vision_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[4]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 6, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_night_vision_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[4]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 5, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_poison_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[28]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 26, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_poison_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[28]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 30, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_poison_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[28]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 29, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_regeneration_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[31]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 33, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_regeneration_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[31]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 32, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_slow_falling_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[40]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 41, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_slowness_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[16]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 18, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_slowness_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[16]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 17, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_strength_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[34]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 36, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_strength_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[34]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 35, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_strong_healing_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[25]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 27, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_strong_poison_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[30]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 27, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_swiftness_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[13]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 16, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_swiftness_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[13]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 15, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_swiftness_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[13]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 14, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_turtle_master_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[19]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 21, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_turtle_master_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[19]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 20, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_water_blaze_powder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_86, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_water_breathing_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[22]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 23, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_water_breeze_rod", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_87, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_water_cobweb", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_88, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_water_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 37, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_water_ghast_tear", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_89, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_water_glistering_melon_slice", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_90, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_water_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 2, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_water_magma_cream", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_92, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_water_nether_wart", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_104, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 3, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_water_rabbit_foot", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_95, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_water_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_water_slime_block", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_96, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_water_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_97, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_water_stone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_98, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_water_sugar", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_99, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/lingering_potion_weakness_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_85, potions: Some(&[37]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 38, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_awkward_blaze_powder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_86, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 34, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_awkward_breeze_rod", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_87, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 42, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_awkward_cobweb", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_88, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 43, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_awkward_ghast_tear", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_89, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 31, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_awkward_glistering_melon_slice", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_90, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 24, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_awkward_golden_carrot", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_91, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 4, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_awkward_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 3, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_awkward_magma_cream", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_92, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 11, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_awkward_phantom_membrane", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_93, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 40, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_awkward_pufferfish", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_94, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 22, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_awkward_rabbit_foot", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_95, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 8, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_awkward_slime_block", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_96, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 44, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_awkward_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_97, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 28, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_awkward_stone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_98, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 45, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_awkward_sugar", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_99, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 13, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_awkward_turtle_helmet", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_100, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 19, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_fire_resistance_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[11]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 11, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_fire_resistance_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[11]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 12, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_harming_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[26]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 27, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_harming_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[26]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 26, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_healing_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[24]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 26, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_healing_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[24]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 25, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_healing_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[24]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 24, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_infested_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[45]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 45, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_invisibility_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[6]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 6, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_invisibility_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[6]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 7, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_leaping_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[8]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 16, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_leaping_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[8]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 10, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_leaping_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[8]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 8, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_leaping_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[8]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 9, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_long_fire_resistance_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[12]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 12, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_long_invisibility_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[7]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 7, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_long_leaping_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[9]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 17, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_long_leaping_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[9]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 9, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_long_night_vision_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[5]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 7, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_long_night_vision_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[5]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 5, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_long_poison_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[29]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 26, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_long_poison_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[29]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 29, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_long_regeneration_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[32]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 32, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_long_slow_falling_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[41]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 41, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_long_slowness_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[17]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 17, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_long_strength_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[35]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 35, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_long_swiftness_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[14]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 17, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_long_swiftness_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[14]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 14, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_long_turtle_master_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[20]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 20, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_long_water_breathing_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[23]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 23, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_long_weakness_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[38]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 38, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_mundane_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[1]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_night_vision_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[4]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 6, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_night_vision_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[4]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 4, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_night_vision_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[4]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 5, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_oozing_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[44]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 44, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_poison_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[28]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 26, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_poison_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[28]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 30, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_poison_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[28]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 28, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_poison_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[28]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 29, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_regeneration_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[31]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 33, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_regeneration_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[31]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 31, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_regeneration_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[31]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 32, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_slow_falling_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[40]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 40, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_slow_falling_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[40]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 41, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_slowness_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[16]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 18, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_slowness_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[16]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 16, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_slowness_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[16]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 17, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_strength_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[34]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 36, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_strength_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[34]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 34, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_strength_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[34]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 35, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_strong_harming_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[27]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 27, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_strong_healing_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[25]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 27, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_strong_healing_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[25]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 25, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_strong_leaping_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[10]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 10, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_strong_poison_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[30]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 27, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_strong_poison_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[30]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 30, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_strong_regeneration_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[33]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 33, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_strong_slowness_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[18]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 18, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_strong_strength_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[36]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 36, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_strong_swiftness_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[15]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 15, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_strong_turtle_master_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[21]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 21, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_swiftness_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[13]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 16, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_swiftness_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[13]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 15, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_swiftness_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[13]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 13, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_swiftness_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[13]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 14, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_thick_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[2]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 2, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_turtle_master_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[19]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 21, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_turtle_master_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[19]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 19, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_turtle_master_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[19]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 20, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_water_blaze_powder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_86, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_water_breathing_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[22]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 22, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_water_breathing_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[22]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 23, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_water_breeze_rod", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_87, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_water_cobweb", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_88, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_water_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 37, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_water_ghast_tear", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_89, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_water_glistering_melon_slice", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_90, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_water_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 2, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_water_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 0, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_water_magma_cream", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_92, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_water_nether_wart", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_104, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 3, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_water_rabbit_foot", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_95, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_water_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_water_slime_block", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_96, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_water_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_97, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_water_stone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_98, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_water_sugar", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_99, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_weakness_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[37]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 37, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_weakness_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[37]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1271, count: 1, components: &[1, 0, 53, 1, 38, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_weaving_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[43]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 43, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/potion_wind_charged_gunpowder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_105, potions: Some(&[42]) }, reagent: PotionIngredient { item: INGREDIENT_106, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 42, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_awkward_blaze_powder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_86, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 34, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_awkward_breeze_rod", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_87, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 42, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_awkward_cobweb", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_88, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 43, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_awkward_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 3, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_awkward_ghast_tear", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_89, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 31, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_awkward_glistering_melon_slice", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_90, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 24, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_awkward_golden_carrot", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_91, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 4, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_awkward_magma_cream", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_92, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 11, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_awkward_phantom_membrane", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_93, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 40, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_awkward_pufferfish", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_94, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 22, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_awkward_rabbit_foot", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_95, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 8, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_awkward_slime_block", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_96, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 44, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_awkward_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_97, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 28, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_awkward_stone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_98, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 45, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_awkward_sugar", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_99, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 13, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_awkward_turtle_helmet", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[3]) }, reagent: PotionIngredient { item: INGREDIENT_100, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 19, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_fire_resistance_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[11]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 11, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_fire_resistance_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[11]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 12, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_harming_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[26]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 26, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_harming_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[26]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 27, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_healing_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[24]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 24, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_healing_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[24]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 26, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_healing_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[24]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 25, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_infested_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[45]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 45, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_invisibility_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[6]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 6, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_invisibility_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[6]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 7, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_leaping_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[8]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 8, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_leaping_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[8]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 16, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_leaping_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[8]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 10, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_leaping_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[8]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 9, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_long_fire_resistance_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[12]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 12, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_long_invisibility_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[7]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 7, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_long_leaping_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[9]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 9, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_long_leaping_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[9]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 17, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_long_night_vision_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[5]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 5, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_long_night_vision_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[5]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 7, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_long_poison_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[29]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 29, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_long_poison_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[29]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 26, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_long_regeneration_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[32]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 32, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_long_slow_falling_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[41]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 41, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_long_slowness_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[17]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 17, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_long_strength_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[35]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 35, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_long_swiftness_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[14]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 14, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_long_swiftness_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[14]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 17, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_long_turtle_master_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[20]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 20, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_long_water_breathing_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[23]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 23, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_long_weakness_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[38]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 38, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_mundane_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[1]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_night_vision_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[4]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 4, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_night_vision_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[4]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 6, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_night_vision_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[4]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 5, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_oozing_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[44]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 44, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_poison_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[28]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 28, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_poison_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[28]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 26, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_poison_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[28]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 30, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_poison_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[28]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 29, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_regeneration_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[31]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 31, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_regeneration_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[31]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 33, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_regeneration_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[31]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 32, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_slow_falling_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[40]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 40, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_slow_falling_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[40]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 41, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_slowness_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[16]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 16, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_slowness_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[16]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 18, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_slowness_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[16]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 17, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_strength_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[34]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 34, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_strength_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[34]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 36, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_strength_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[34]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 35, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_strong_harming_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[27]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 27, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_strong_healing_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[25]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 25, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_strong_healing_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[25]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 27, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_strong_leaping_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[10]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 10, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_strong_poison_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[30]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 30, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_strong_poison_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[30]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 27, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_strong_regeneration_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[33]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 33, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_strong_slowness_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[18]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 18, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_strong_strength_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[36]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 36, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_strong_swiftness_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[15]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 15, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_strong_turtle_master_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[21]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 21, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_swiftness_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[13]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 13, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_swiftness_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[13]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 16, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_swiftness_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[13]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 15, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_swiftness_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[13]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 14, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_thick_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[2]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 2, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_turtle_master_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[19]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 19, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_turtle_master_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[19]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 21, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_turtle_master_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[19]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 20, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_water_blaze_powder", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_86, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_water_breathing_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[22]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 22, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_water_breathing_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[22]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 23, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_water_breeze_rod", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_87, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_water_cobweb", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_88, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_water_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 0, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_water_fermented_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_103, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 37, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_water_ghast_tear", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_89, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_water_glistering_melon_slice", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_90, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_water_glowstone_dust", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_102, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 2, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_water_magma_cream", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_92, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_water_nether_wart", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_104, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 3, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_water_rabbit_foot", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_95, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_water_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_water_slime_block", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_96, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_water_spider_eye", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_97, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_water_stone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_98, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_water_sugar", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[0]) }, reagent: PotionIngredient { item: INGREDIENT_99, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 1, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_weakness_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[37]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 37, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_weakness_redstone", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[37]) }, reagent: PotionIngredient { item: INGREDIENT_101, potions: None }, result: Stack { item: 1442, count: 1, components: &[1, 0, 53, 1, 38, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_weaving_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[43]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 43, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing/splash_potion_wind_charged_dragon_breath", kind: RecipeKind::Brewing { input: PotionIngredient { item: INGREDIENT_107, potions: Some(&[42]) }, reagent: PotionIngredient { item: INGREDIENT_108, potions: None }, result: Stack { item: 1445, count: 1, components: &[1, 0, 53, 1, 42, 0, 0, 0] } } },
    Recipe { id: "minecraft:brewing_stand", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_82, result: Stack { item: 1276, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:brick", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_110, result: Stack { item: 1142, count: 1, components: &[0, 0] }, experience: 0.3, time: 200 } },
    Recipe { id: "minecraft:brick_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_83, result: Stack { item: 361, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:brick_slab_from_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_111, result: Stack { item: 361, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:brick_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_84, result: Stack { item: 493, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:brick_stairs_from_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_111, result: Stack { item: 493, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:brick_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_85, result: Stack { item: 533, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:brick_wall_from_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_111, result: Stack { item: 533, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:bricks", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_86, result: Stack { item: 376, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:brown_banner", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_87, result: Stack { item: 1429, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:brown_banner_duplicate", kind: RecipeKind::BannerDuplicate { banner: INGREDIENT_114, result: Stack { item: 1429, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:brown_bed", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_88, result: Stack { item: 1231, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:brown_bundle", kind: RecipeKind::Transmute { input: INGREDIENT_41, material: INGREDIENT_115, min_material: 1, max_material: 1, result_item: Some(1166), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:brown_candle", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_26, result: Stack { item: 1563, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:brown_carpet", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_89, result: Stack { item: 592, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:brown_concrete_powder", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_27, result: Stack { item: 749, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:brown_concrete_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_90, result: Stack { item: 733, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:brown_concrete_slab_from_brown_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_116, result: Stack { item: 733, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:brown_concrete_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_91, result: Stack { item: 717, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:brown_concrete_stairs_from_brown_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_116, result: Stack { item: 717, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:brown_cushion", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_92, result: Stack { item: 1182, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:brown_dye", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_28, result: Stack { item: 1211, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:brown_glazed_terracotta", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_119, result: Stack { item: 685, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:brown_harness", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_93, result: Stack { item: 962, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:brown_shulker_box", kind: RecipeKind::Transmute { input: INGREDIENT_52, material: INGREDIENT_115, min_material: 1, max_material: 1, result_item: Some(669), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:brown_stained_glass", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_94, result: Stack { item: 617, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:brown_stained_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_95, result: Stack { item: 633, count: 16, components: &[0, 0] } } },
    Recipe { id: "minecraft:brown_stained_glass_pane_from_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_96, result: Stack { item: 633, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:brown_terracotta", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_97, result: Stack { item: 573, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:brown_wool_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_98, result: Stack { item: 294, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:brown_wool_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_99, result: Stack { item: 278, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:brush", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_100, result: Stack { item: 1578, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:bucket", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_101, result: Stack { item: 1128, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:bundle", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_102, result: Stack { item: 1153, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cake", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_103, result: Stack { item: 1218, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:calibrated_sculk_sensor", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_104, result: Stack { item: 850, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:campfire", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_105, result: Stack { item: 1527, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:candle", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_106, result: Stack { item: 1550, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:carrot_on_a_stick", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_107, result: Stack { item: 971, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cartography_table", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_108, result: Stack { item: 1509, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cauldron", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_109, result: Stack { item: 1277, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:charcoal", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_129, result: Stack { item: 1011, count: 1, components: &[0, 0] }, experience: 0.15, time: 200 } },
    Recipe { id: "minecraft:cherry_boat", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_110, result: Stack { item: 985, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cherry_button", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_29, result: Stack { item: 863, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cherry_chest_boat", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_30, result: Stack { item: 986, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cherry_door", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_111, result: Stack { item: 894, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:cherry_fence", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_112, result: Stack { item: 422, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:cherry_fence_gate", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_113, result: Stack { item: 937, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cherry_hanging_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_114, result: Stack { item: 1120, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:cherry_planks", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_31, result: Stack { item: 68, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:cherry_pressure_plate", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_115, result: Stack { item: 880, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cherry_shelf", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_116, result: Stack { item: 380, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:cherry_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_117, result: Stack { item: 1107, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:cherry_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_118, result: Stack { item: 346, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:cherry_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_119, result: Stack { item: 520, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:cherry_trapdoor", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_111, result: Stack { item: 916, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:cherry_wood", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_120, result: Stack { item: 208, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:chest", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_121, result: Stack { item: 404, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chest_minecart", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_32, result: Stack { item: 967, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_bookshelf", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_122, result: Stack { item: 391, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_cinnabar", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_123, result: Stack { item: 52, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_cinnabar_from_cinnabar_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_137, result: Stack { item: 52, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_copper", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_124, result: Stack { item: 131, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_copper_from_copper_block_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_139, result: Stack { item: 131, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_copper_from_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_140, result: Stack { item: 131, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_deepslate", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_125, result: Stack { item: 459, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_deepslate_from_cobbled_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_142, result: Stack { item: 459, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_deepslate_from_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_143, result: Stack { item: 459, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_nether_bricks", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_126, result: Stack { item: 500, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_nether_bricks_from_nether_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_145, result: Stack { item: 500, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_polished_blackstone", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_127, result: Stack { item: 1544, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_polished_blackstone_from_blackstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_56, result: Stack { item: 1544, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_polished_blackstone_from_polished_blackstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_147, result: Stack { item: 1544, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_quartz_block", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_128, result: Stack { item: 556, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_quartz_block_from_quartz_block_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_149, result: Stack { item: 556, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_red_sandstone", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_129, result: Stack { item: 645, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_red_sandstone_from_red_sandstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_151, result: Stack { item: 645, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_resin_bricks", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_130, result: Stack { item: 492, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_resin_bricks_from_resin_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_153, result: Stack { item: 492, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_sandstone", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_131, result: Stack { item: 235, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_sandstone_from_sandstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_155, result: Stack { item: 235, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_stone_bricks", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_132, result: Stack { item: 452, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_stone_bricks_from_stone_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_157, result: Stack { item: 452, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_stone_bricks_from_stone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_98, result: Stack { item: 452, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_sulfur", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_133, result: Stack { item: 39, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_sulfur_from_sulfur_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_159, result: Stack { item: 39, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_tuff", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_134, result: Stack { item: 16, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_tuff_bricks", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_135, result: Stack { item: 25, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_tuff_bricks_from_polished_tuff_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_162, result: Stack { item: 25, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_tuff_bricks_from_tuff_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_163, result: Stack { item: 25, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_tuff_bricks_from_tuff_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_164, result: Stack { item: 25, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:chiseled_tuff_from_tuff_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_164, result: Stack { item: 16, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cinnabar_brick_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_136, result: Stack { item: 49, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:cinnabar_brick_slab_from_cinnabar_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_165, result: Stack { item: 49, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:cinnabar_brick_slab_from_cinnabar_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_137, result: Stack { item: 49, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:cinnabar_brick_slab_from_polished_cinnabar_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_166, result: Stack { item: 49, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:cinnabar_brick_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_137, result: Stack { item: 50, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:cinnabar_brick_stairs_from_cinnabar_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_165, result: Stack { item: 50, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cinnabar_brick_stairs_from_cinnabar_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_137, result: Stack { item: 50, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cinnabar_brick_stairs_from_polished_cinnabar_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_166, result: Stack { item: 50, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cinnabar_brick_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_138, result: Stack { item: 51, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:cinnabar_brick_wall_from_cinnabar_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_165, result: Stack { item: 51, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cinnabar_brick_wall_from_cinnabar_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_137, result: Stack { item: 51, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cinnabar_brick_wall_from_polished_cinnabar_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_166, result: Stack { item: 51, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cinnabar_bricks", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_139, result: Stack { item: 48, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:cinnabar_bricks_from_cinnabar_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_137, result: Stack { item: 48, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cinnabar_bricks_from_polished_cinnabar_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_166, result: Stack { item: 48, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cinnabar_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_140, result: Stack { item: 41, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:cinnabar_slab_from_cinnabar_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_137, result: Stack { item: 41, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:cinnabar_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_141, result: Stack { item: 42, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:cinnabar_stairs_from_cinnabar_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_137, result: Stack { item: 42, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cinnabar_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_142, result: Stack { item: 43, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:cinnabar_wall_from_cinnabar_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_137, result: Stack { item: 43, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:clay", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_143, result: Stack { item: 415, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:clock", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_144, result: Stack { item: 1187, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:coal", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_33, result: Stack { item: 1010, count: 9, components: &[0, 0] } } },
    Recipe { id: "minecraft:coal_block", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_145, result: Stack { item: 112, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:coal_from_blasting_coal_ore", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_170, result: Stack { item: 1010, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:coal_from_blasting_deepslate_coal_ore", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_171, result: Stack { item: 1010, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:coal_from_smelting_coal_ore", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_170, result: Stack { item: 1010, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:coal_from_smelting_deepslate_coal_ore", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_171, result: Stack { item: 1010, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:coarse_dirt", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_146, result: Stack { item: 56, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:coast_armor_trim_smithing_template", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_147, result: Stack { item: 1582, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:coast_armor_trim_smithing_template_smithing_trim", kind: RecipeKind::SmithingTrim { template: INGREDIENT_173, base: INGREDIENT_73, addition: INGREDIENT_74, pattern: "minecraft:coast" } },
    Recipe { id: "minecraft:cobbled_deepslate_from_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_143, result: Stack { item: 9, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cobbled_deepslate_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_148, result: Stack { item: 819, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:cobbled_deepslate_slab_from_cobbled_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_142, result: Stack { item: 819, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:cobbled_deepslate_slab_from_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_143, result: Stack { item: 819, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:cobbled_deepslate_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_149, result: Stack { item: 802, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:cobbled_deepslate_stairs_from_cobbled_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_142, result: Stack { item: 802, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cobbled_deepslate_stairs_from_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_143, result: Stack { item: 802, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cobbled_deepslate_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_150, result: Stack { item: 549, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:cobbled_deepslate_wall_from_cobbled_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_142, result: Stack { item: 549, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cobbled_deepslate_wall_from_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_143, result: Stack { item: 549, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cobblestone_from_stone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_98, result: Stack { item: 62, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cobblestone_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_151, result: Stack { item: 360, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:cobblestone_slab_from_cobblestone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_12, result: Stack { item: 360, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:cobblestone_slab_from_stone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_98, result: Stack { item: 360, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:cobblestone_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_152, result: Stack { item: 409, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:cobblestone_stairs_from_cobblestone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_12, result: Stack { item: 409, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cobblestone_stairs_from_stone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_98, result: Stack { item: 409, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cobblestone_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_153, result: Stack { item: 531, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:cobblestone_wall_from_cobblestone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_12, result: Stack { item: 531, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cobblestone_wall_from_stone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_98, result: Stack { item: 531, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:comparator", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_154, result: Stack { item: 828, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:compass", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_155, result: Stack { item: 1151, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:composter", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_156, result: Stack { item: 1505, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:conduit", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_157, result: Stack { item: 787, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cooked_beef", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_177, result: Stack { item: 1261, count: 1, components: &[0, 0] }, experience: 0.35, time: 200 } },
    Recipe { id: "minecraft:cooked_beef_from_campfire_cooking", kind: RecipeKind::Cooking { kind: CookingKind::Campfire, ingredient: INGREDIENT_177, result: Stack { item: 1261, count: 1, components: &[0, 0] }, experience: 0.35, time: 600 } },
    Recipe { id: "minecraft:cooked_beef_from_smoking", kind: RecipeKind::Cooking { kind: CookingKind::Smoking, ingredient: INGREDIENT_177, result: Stack { item: 1261, count: 1, components: &[0, 0] }, experience: 0.35, time: 200 } },
    Recipe { id: "minecraft:cooked_chicken", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_178, result: Stack { item: 1263, count: 1, components: &[0, 0] }, experience: 0.35, time: 200 } },
    Recipe { id: "minecraft:cooked_chicken_from_campfire_cooking", kind: RecipeKind::Cooking { kind: CookingKind::Campfire, ingredient: INGREDIENT_178, result: Stack { item: 1263, count: 1, components: &[0, 0] }, experience: 0.35, time: 600 } },
    Recipe { id: "minecraft:cooked_chicken_from_smoking", kind: RecipeKind::Cooking { kind: CookingKind::Smoking, ingredient: INGREDIENT_178, result: Stack { item: 1263, count: 1, components: &[0, 0] }, experience: 0.35, time: 200 } },
    Recipe { id: "minecraft:cooked_cod", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_179, result: Stack { item: 1194, count: 1, components: &[0, 0] }, experience: 0.35, time: 200 } },
    Recipe { id: "minecraft:cooked_cod_from_campfire_cooking", kind: RecipeKind::Cooking { kind: CookingKind::Campfire, ingredient: INGREDIENT_179, result: Stack { item: 1194, count: 1, components: &[0, 0] }, experience: 0.35, time: 600 } },
    Recipe { id: "minecraft:cooked_cod_from_smoking", kind: RecipeKind::Cooking { kind: CookingKind::Smoking, ingredient: INGREDIENT_179, result: Stack { item: 1194, count: 1, components: &[0, 0] }, experience: 0.35, time: 200 } },
    Recipe { id: "minecraft:cooked_mutton", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_180, result: Stack { item: 1416, count: 1, components: &[0, 0] }, experience: 0.35, time: 200 } },
    Recipe { id: "minecraft:cooked_mutton_from_campfire_cooking", kind: RecipeKind::Cooking { kind: CookingKind::Campfire, ingredient: INGREDIENT_180, result: Stack { item: 1416, count: 1, components: &[0, 0] }, experience: 0.35, time: 600 } },
    Recipe { id: "minecraft:cooked_mutton_from_smoking", kind: RecipeKind::Cooking { kind: CookingKind::Smoking, ingredient: INGREDIENT_180, result: Stack { item: 1416, count: 1, components: &[0, 0] }, experience: 0.35, time: 200 } },
    Recipe { id: "minecraft:cooked_porkchop", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_181, result: Stack { item: 1098, count: 1, components: &[0, 0] }, experience: 0.35, time: 200 } },
    Recipe { id: "minecraft:cooked_porkchop_from_campfire_cooking", kind: RecipeKind::Cooking { kind: CookingKind::Campfire, ingredient: INGREDIENT_181, result: Stack { item: 1098, count: 1, components: &[0, 0] }, experience: 0.35, time: 600 } },
    Recipe { id: "minecraft:cooked_porkchop_from_smoking", kind: RecipeKind::Cooking { kind: CookingKind::Smoking, ingredient: INGREDIENT_181, result: Stack { item: 1098, count: 1, components: &[0, 0] }, experience: 0.35, time: 200 } },
    Recipe { id: "minecraft:cooked_rabbit", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_182, result: Stack { item: 1401, count: 1, components: &[0, 0] }, experience: 0.35, time: 200 } },
    Recipe { id: "minecraft:cooked_rabbit_from_campfire_cooking", kind: RecipeKind::Cooking { kind: CookingKind::Campfire, ingredient: INGREDIENT_182, result: Stack { item: 1401, count: 1, components: &[0, 0] }, experience: 0.35, time: 600 } },
    Recipe { id: "minecraft:cooked_rabbit_from_smoking", kind: RecipeKind::Cooking { kind: CookingKind::Smoking, ingredient: INGREDIENT_182, result: Stack { item: 1401, count: 1, components: &[0, 0] }, experience: 0.35, time: 200 } },
    Recipe { id: "minecraft:cooked_salmon", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_183, result: Stack { item: 1195, count: 1, components: &[0, 0] }, experience: 0.35, time: 200 } },
    Recipe { id: "minecraft:cooked_salmon_from_campfire_cooking", kind: RecipeKind::Cooking { kind: CookingKind::Campfire, ingredient: INGREDIENT_183, result: Stack { item: 1195, count: 1, components: &[0, 0] }, experience: 0.35, time: 600 } },
    Recipe { id: "minecraft:cooked_salmon_from_smoking", kind: RecipeKind::Cooking { kind: CookingKind::Smoking, ingredient: INGREDIENT_183, result: Stack { item: 1195, count: 1, components: &[0, 0] }, experience: 0.35, time: 200 } },
    Recipe { id: "minecraft:cookie", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_158, result: Stack { item: 1236, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_axe", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_159, result: Stack { item: 1033, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_bars", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_160, result: Stack { item: 465, count: 16, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_block", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_161, result: Stack { item: 120, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_boots", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_162, result: Stack { item: 1075, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_bulb", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_163, result: Stack { item: 1629, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_chain", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_164, result: Stack { item: 474, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_chest", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_165, result: Stack { item: 1637, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_chestplate", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_166, result: Stack { item: 1073, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_door", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_160, result: Stack { item: 902, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_grate", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_167, result: Stack { item: 1621, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_grate_from_copper_block_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_139, result: Stack { item: 1621, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_helmet", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_168, result: Stack { item: 1072, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_hoe", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_169, result: Stack { item: 1034, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_ingot", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_34, result: Stack { item: 1020, count: 9, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_ingot_from_blasting_copper_ore", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_186, result: Stack { item: 1020, count: 1, components: &[0, 0] }, experience: 0.7, time: 200 } },
    Recipe { id: "minecraft:copper_ingot_from_blasting_deepslate_copper_ore", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_187, result: Stack { item: 1020, count: 1, components: &[0, 0] }, experience: 0.7, time: 200 } },
    Recipe { id: "minecraft:copper_ingot_from_blasting_raw_copper", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_188, result: Stack { item: 1020, count: 1, components: &[0, 0] }, experience: 0.7, time: 200 } },
    Recipe { id: "minecraft:copper_ingot_from_nuggets", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_170, result: Stack { item: 1020, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_ingot_from_smelting_copper_ore", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_186, result: Stack { item: 1020, count: 1, components: &[0, 0] }, experience: 0.7, time: 200 } },
    Recipe { id: "minecraft:copper_ingot_from_smelting_deepslate_copper_ore", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_187, result: Stack { item: 1020, count: 1, components: &[0, 0] }, experience: 0.7, time: 200 } },
    Recipe { id: "minecraft:copper_ingot_from_smelting_raw_copper", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_188, result: Stack { item: 1020, count: 1, components: &[0, 0] }, experience: 0.7, time: 200 } },
    Recipe { id: "minecraft:copper_ingot_from_waxed_copper_block", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_35, result: Stack { item: 1020, count: 9, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_lantern", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_171, result: Stack { item: 1517, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_leggings", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_172, result: Stack { item: 1074, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_nugget", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_36, result: Stack { item: 1457, count: 9, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_nugget_from_blasting", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_191, result: Stack { item: 1457, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:copper_nugget_from_smelting", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_191, result: Stack { item: 1457, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:copper_pickaxe", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_173, result: Stack { item: 1032, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_shovel", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_174, result: Stack { item: 1031, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_spear", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_175, result: Stack { item: 1449, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_sword", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_176, result: Stack { item: 1030, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_torch", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_177, result: Stack { item: 440, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:copper_trapdoor", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_178, result: Stack { item: 924, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cracked_deepslate_bricks", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_193, result: Stack { item: 456, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:cracked_deepslate_tiles", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_194, result: Stack { item: 458, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:cracked_nether_bricks", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_145, result: Stack { item: 499, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:cracked_polished_blackstone_bricks", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_195, result: Stack { item: 1548, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:cracked_stone_bricks", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_157, result: Stack { item: 451, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:crafter", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_179, result: Stack { item: 1237, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:crafting_table", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_180, result: Stack { item: 405, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:creaking_heart", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_181, result: Stack { item: 403, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:creeper_banner_pattern", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_37, result: Stack { item: 1495, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:crimson_button", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_38, result: Stack { item: 869, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:crimson_door", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_182, result: Stack { item: 900, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:crimson_fence", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_183, result: Stack { item: 428, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:crimson_fence_gate", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_184, result: Stack { item: 943, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:crimson_hanging_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_185, result: Stack { item: 1126, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:crimson_hyphae", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_186, result: Stack { item: 213, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:crimson_planks", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_39, result: Stack { item: 74, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:crimson_pressure_plate", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_187, result: Stack { item: 886, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:crimson_shelf", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_188, result: Stack { item: 381, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:crimson_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_189, result: Stack { item: 1113, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:crimson_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_190, result: Stack { item: 353, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:crimson_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_191, result: Stack { item: 527, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:crimson_trapdoor", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_182, result: Stack { item: 922, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:crossbow", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_192, result: Stack { item: 1491, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cut_copper", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_193, result: Stack { item: 139, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:cut_copper_from_copper_block_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_139, result: Stack { item: 139, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:cut_copper_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_194, result: Stack { item: 155, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:cut_copper_slab_from_copper_block_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_139, result: Stack { item: 155, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:cut_copper_slab_from_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_140, result: Stack { item: 155, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:cut_copper_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_195, result: Stack { item: 147, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:cut_copper_stairs_from_copper_block_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_139, result: Stack { item: 147, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:cut_copper_stairs_from_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_140, result: Stack { item: 147, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cut_red_sandstone", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_196, result: Stack { item: 646, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:cut_red_sandstone_from_red_sandstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_151, result: Stack { item: 646, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cut_red_sandstone_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_197, result: Stack { item: 367, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:cut_red_sandstone_slab_from_cut_red_sandstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_206, result: Stack { item: 367, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:cut_red_sandstone_slab_from_red_sandstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_151, result: Stack { item: 367, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:cut_sandstone", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_198, result: Stack { item: 236, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:cut_sandstone_from_sandstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_155, result: Stack { item: 236, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cut_sandstone_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_199, result: Stack { item: 358, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:cut_sandstone_slab_from_cut_sandstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_207, result: Stack { item: 358, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:cut_sandstone_slab_from_sandstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_155, result: Stack { item: 358, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:cyan_banner", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_200, result: Stack { item: 1426, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cyan_banner_duplicate", kind: RecipeKind::BannerDuplicate { banner: INGREDIENT_209, result: Stack { item: 1426, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cyan_bed", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_201, result: Stack { item: 1228, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cyan_bundle", kind: RecipeKind::Transmute { input: INGREDIENT_41, material: INGREDIENT_210, min_material: 1, max_material: 1, result_item: Some(1163), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:cyan_candle", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_40, result: Stack { item: 1560, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cyan_carpet", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_202, result: Stack { item: 589, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:cyan_concrete_powder", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_41, result: Stack { item: 746, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:cyan_concrete_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_203, result: Stack { item: 730, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:cyan_concrete_slab_from_cyan_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_211, result: Stack { item: 730, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:cyan_concrete_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_204, result: Stack { item: 714, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:cyan_concrete_stairs_from_cyan_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_211, result: Stack { item: 714, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cyan_cushion", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_205, result: Stack { item: 1179, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cyan_dye", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_42, result: Stack { item: 1208, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:cyan_dye_from_pitcher_plant", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_43, result: Stack { item: 1208, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:cyan_glazed_terracotta", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_215, result: Stack { item: 682, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:cyan_harness", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_206, result: Stack { item: 959, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:cyan_shulker_box", kind: RecipeKind::Transmute { input: INGREDIENT_52, material: INGREDIENT_210, min_material: 1, max_material: 1, result_item: Some(666), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:cyan_stained_glass", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_207, result: Stack { item: 614, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:cyan_stained_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_208, result: Stack { item: 630, count: 16, components: &[0, 0] } } },
    Recipe { id: "minecraft:cyan_stained_glass_pane_from_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_209, result: Stack { item: 630, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:cyan_terracotta", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_210, result: Stack { item: 570, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:cyan_wool_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_211, result: Stack { item: 291, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:cyan_wool_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_212, result: Stack { item: 275, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:dark_oak_boat", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_213, result: Stack { item: 987, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dark_oak_button", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_44, result: Stack { item: 864, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dark_oak_chest_boat", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_45, result: Stack { item: 988, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dark_oak_door", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_214, result: Stack { item: 895, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:dark_oak_fence", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_215, result: Stack { item: 423, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:dark_oak_fence_gate", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_216, result: Stack { item: 938, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dark_oak_hanging_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_217, result: Stack { item: 1121, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:dark_oak_planks", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_46, result: Stack { item: 69, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:dark_oak_pressure_plate", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_218, result: Stack { item: 881, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dark_oak_shelf", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_219, result: Stack { item: 382, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:dark_oak_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_220, result: Stack { item: 1108, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:dark_oak_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_221, result: Stack { item: 347, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:dark_oak_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_222, result: Stack { item: 521, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:dark_oak_trapdoor", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_214, result: Stack { item: 917, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:dark_oak_wood", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_223, result: Stack { item: 210, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:dark_prismarine", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_224, result: Stack { item: 639, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dark_prismarine_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_225, result: Stack { item: 371, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:dark_prismarine_slab_from_dark_prismarine_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_223, result: Stack { item: 371, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:dark_prismarine_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_226, result: Stack { item: 642, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:dark_prismarine_stairs_from_dark_prismarine_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_223, result: Stack { item: 642, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:daylight_detector", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_227, result: Stack { item: 848, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:decorated_pot", kind: RecipeKind::DecoratedPot { back: INGREDIENT_224, left: INGREDIENT_224, right: INGREDIENT_224, front: INGREDIENT_224, result: Stack { item: 392, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:decorated_pot_simple", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_228, result: Stack { item: 392, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_142, result: Stack { item: 8, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:deepslate_brick_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_229, result: Stack { item: 821, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_brick_slab_from_cobbled_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_142, result: Stack { item: 821, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_brick_slab_from_deepslate_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_193, result: Stack { item: 821, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_brick_slab_from_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_143, result: Stack { item: 821, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_brick_slab_from_polished_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_225, result: Stack { item: 821, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_brick_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_230, result: Stack { item: 804, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_brick_stairs_from_cobbled_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_142, result: Stack { item: 804, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_brick_stairs_from_deepslate_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_193, result: Stack { item: 804, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_brick_stairs_from_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_143, result: Stack { item: 804, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_brick_stairs_from_polished_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_225, result: Stack { item: 804, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_brick_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_231, result: Stack { item: 551, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_brick_wall_from_cobbled_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_142, result: Stack { item: 551, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_brick_wall_from_deepslate_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_193, result: Stack { item: 551, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_brick_wall_from_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_143, result: Stack { item: 551, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_brick_wall_from_polished_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_225, result: Stack { item: 551, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_bricks", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_232, result: Stack { item: 455, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_bricks_from_cobbled_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_142, result: Stack { item: 455, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_bricks_from_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_143, result: Stack { item: 455, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_bricks_from_polished_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_225, result: Stack { item: 455, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tile_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_233, result: Stack { item: 822, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tile_slab_from_cobbled_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_142, result: Stack { item: 822, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tile_slab_from_deepslate_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_193, result: Stack { item: 822, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tile_slab_from_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_143, result: Stack { item: 822, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tile_slab_from_deepslate_tiles_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_194, result: Stack { item: 822, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tile_slab_from_polished_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_225, result: Stack { item: 822, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tile_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_234, result: Stack { item: 805, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tile_stairs_from_cobbled_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_142, result: Stack { item: 805, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tile_stairs_from_deepslate_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_193, result: Stack { item: 805, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tile_stairs_from_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_143, result: Stack { item: 805, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tile_stairs_from_deepslate_tiles_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_194, result: Stack { item: 805, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tile_stairs_from_polished_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_225, result: Stack { item: 805, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tile_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_235, result: Stack { item: 552, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tile_wall_from_cobbled_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_142, result: Stack { item: 552, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tile_wall_from_deepslate_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_193, result: Stack { item: 552, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tile_wall_from_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_143, result: Stack { item: 552, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tile_wall_from_deepslate_tiles_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_194, result: Stack { item: 552, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tile_wall_from_polished_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_225, result: Stack { item: 552, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tiles", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_236, result: Stack { item: 457, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tiles_from_cobbled_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_142, result: Stack { item: 457, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tiles_from_deepslate_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_193, result: Stack { item: 457, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tiles_from_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_143, result: Stack { item: 457, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:deepslate_tiles_from_polished_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_225, result: Stack { item: 457, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:detector_rail", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_237, result: Stack { item: 946, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:diamond", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_47, result: Stack { item: 1012, count: 9, components: &[0, 0] } } },
    Recipe { id: "minecraft:diamond_axe", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_238, result: Stack { item: 1053, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:diamond_block", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_239, result: Stack { item: 129, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:diamond_boots", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_240, result: Stack { item: 1087, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:diamond_chestplate", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_241, result: Stack { item: 1085, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:diamond_from_blasting_deepslate_diamond_ore", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_229, result: Stack { item: 1012, count: 1, components: &[0, 0] }, experience: 1.0, time: 200 } },
    Recipe { id: "minecraft:diamond_from_blasting_diamond_ore", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_230, result: Stack { item: 1012, count: 1, components: &[0, 0] }, experience: 1.0, time: 200 } },
    Recipe { id: "minecraft:diamond_from_smelting_deepslate_diamond_ore", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_229, result: Stack { item: 1012, count: 1, components: &[0, 0] }, experience: 1.0, time: 200 } },
    Recipe { id: "minecraft:diamond_from_smelting_diamond_ore", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_230, result: Stack { item: 1012, count: 1, components: &[0, 0] }, experience: 1.0, time: 200 } },
    Recipe { id: "minecraft:diamond_helmet", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_242, result: Stack { item: 1084, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:diamond_hoe", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_243, result: Stack { item: 1054, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:diamond_leggings", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_244, result: Stack { item: 1086, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:diamond_pickaxe", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_245, result: Stack { item: 1052, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:diamond_shovel", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_246, result: Stack { item: 1051, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:diamond_spear", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_247, result: Stack { item: 1452, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:diamond_sword", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_248, result: Stack { item: 1050, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:diorite", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_249, result: Stack { item: 4, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:diorite_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_250, result: Stack { item: 818, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:diorite_slab_from_diorite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_11, result: Stack { item: 818, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:diorite_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_251, result: Stack { item: 801, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:diorite_stairs_from_diorite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_11, result: Stack { item: 801, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:diorite_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_252, result: Stack { item: 545, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:diorite_wall_from_diorite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_11, result: Stack { item: 545, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dispenser", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_253, result: Stack { item: 835, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dried_ghast", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_254, result: Stack { item: 755, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dried_kelp", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_48, result: Stack { item: 1257, count: 9, components: &[0, 0] } } },
    Recipe { id: "minecraft:dried_kelp_block", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_255, result: Stack { item: 1144, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dried_kelp_from_campfire_cooking", kind: RecipeKind::Cooking { kind: CookingKind::Campfire, ingredient: INGREDIENT_235, result: Stack { item: 1257, count: 1, components: &[0, 0] }, experience: 0.1, time: 600 } },
    Recipe { id: "minecraft:dried_kelp_from_smelting", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_235, result: Stack { item: 1257, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:dried_kelp_from_smoking", kind: RecipeKind::Cooking { kind: CookingKind::Smoking, ingredient: INGREDIENT_235, result: Stack { item: 1257, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:dripstone_block", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_256, result: Stack { item: 53, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dropper", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_257, result: Stack { item: 836, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dune_armor_trim_smithing_template", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_258, result: Stack { item: 1581, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:dune_armor_trim_smithing_template_smithing_trim", kind: RecipeKind::SmithingTrim { template: INGREDIENT_237, base: INGREDIENT_73, addition: INGREDIENT_74, pattern: "minecraft:dune" } },
    Recipe { id: "minecraft:dye_black_bed", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_49, result: Stack { item: 1234, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_black_carpet", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_50, result: Stack { item: 595, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_black_cushion", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_51, result: Stack { item: 1185, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_black_harness", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_52, result: Stack { item: 965, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_black_wool", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_53, result: Stack { item: 265, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_black_wool_slab", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_54, result: Stack { item: 297, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_black_wool_stairs", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_55, result: Stack { item: 281, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_blue_bed", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_56, result: Stack { item: 1230, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_blue_carpet", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_57, result: Stack { item: 591, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_blue_cushion", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_58, result: Stack { item: 1181, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_blue_harness", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_59, result: Stack { item: 961, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_blue_wool", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_60, result: Stack { item: 261, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_blue_wool_slab", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_61, result: Stack { item: 293, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_blue_wool_stairs", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_62, result: Stack { item: 277, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_brown_bed", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_63, result: Stack { item: 1231, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_brown_carpet", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_64, result: Stack { item: 592, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_brown_cushion", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_65, result: Stack { item: 1182, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_brown_harness", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_66, result: Stack { item: 962, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_brown_wool", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_67, result: Stack { item: 262, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_brown_wool_slab", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_68, result: Stack { item: 294, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_brown_wool_stairs", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_69, result: Stack { item: 278, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_cyan_bed", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_70, result: Stack { item: 1228, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_cyan_carpet", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_71, result: Stack { item: 589, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_cyan_cushion", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_72, result: Stack { item: 1179, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_cyan_harness", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_73, result: Stack { item: 959, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_cyan_wool", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_74, result: Stack { item: 259, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_cyan_wool_slab", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_75, result: Stack { item: 291, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_cyan_wool_stairs", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_76, result: Stack { item: 275, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_gray_bed", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_77, result: Stack { item: 1226, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_gray_carpet", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_78, result: Stack { item: 587, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_gray_cushion", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_79, result: Stack { item: 1177, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_gray_harness", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_80, result: Stack { item: 957, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_gray_wool", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_81, result: Stack { item: 257, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_gray_wool_slab", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_82, result: Stack { item: 289, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_gray_wool_stairs", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_83, result: Stack { item: 273, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_green_bed", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_84, result: Stack { item: 1232, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_green_carpet", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_85, result: Stack { item: 593, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_green_cushion", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_86, result: Stack { item: 1183, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_green_harness", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_87, result: Stack { item: 963, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_green_wool", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_88, result: Stack { item: 263, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_green_wool_slab", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_89, result: Stack { item: 295, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_green_wool_stairs", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_90, result: Stack { item: 279, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_light_blue_bed", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_91, result: Stack { item: 1222, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_light_blue_carpet", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_92, result: Stack { item: 583, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_light_blue_cushion", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_93, result: Stack { item: 1173, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_light_blue_harness", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_94, result: Stack { item: 953, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_light_blue_wool", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_95, result: Stack { item: 253, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_light_blue_wool_slab", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_96, result: Stack { item: 285, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_light_blue_wool_stairs", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_97, result: Stack { item: 269, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_light_gray_bed", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_98, result: Stack { item: 1227, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_light_gray_carpet", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_99, result: Stack { item: 588, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_light_gray_cushion", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_100, result: Stack { item: 1178, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_light_gray_harness", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_101, result: Stack { item: 958, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_light_gray_wool", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_102, result: Stack { item: 258, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_light_gray_wool_slab", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_103, result: Stack { item: 290, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_light_gray_wool_stairs", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_104, result: Stack { item: 274, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_lime_bed", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_105, result: Stack { item: 1224, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_lime_carpet", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_106, result: Stack { item: 585, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_lime_cushion", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_107, result: Stack { item: 1175, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_lime_harness", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_108, result: Stack { item: 955, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_lime_wool", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_109, result: Stack { item: 255, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_lime_wool_slab", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_110, result: Stack { item: 287, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_lime_wool_stairs", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_111, result: Stack { item: 271, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_magenta_bed", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_112, result: Stack { item: 1221, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_magenta_carpet", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_113, result: Stack { item: 582, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_magenta_cushion", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_114, result: Stack { item: 1172, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_magenta_harness", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_115, result: Stack { item: 952, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_magenta_wool", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_116, result: Stack { item: 252, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_magenta_wool_slab", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_117, result: Stack { item: 284, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_magenta_wool_stairs", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_118, result: Stack { item: 268, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_orange_bed", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_119, result: Stack { item: 1220, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_orange_carpet", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_120, result: Stack { item: 581, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_orange_cushion", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_121, result: Stack { item: 1171, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_orange_harness", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_122, result: Stack { item: 951, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_orange_wool", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_123, result: Stack { item: 251, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_orange_wool_slab", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_124, result: Stack { item: 283, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_orange_wool_stairs", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_125, result: Stack { item: 267, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_pink_bed", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_126, result: Stack { item: 1225, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_pink_carpet", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_127, result: Stack { item: 586, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_pink_cushion", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_128, result: Stack { item: 1176, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_pink_harness", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_129, result: Stack { item: 956, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_pink_wool", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_130, result: Stack { item: 256, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_pink_wool_slab", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_131, result: Stack { item: 288, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_pink_wool_stairs", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_132, result: Stack { item: 272, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_purple_bed", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_133, result: Stack { item: 1229, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_purple_carpet", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_134, result: Stack { item: 590, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_purple_cushion", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_135, result: Stack { item: 1180, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_purple_harness", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_136, result: Stack { item: 960, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_purple_wool", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_137, result: Stack { item: 260, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_purple_wool_slab", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_138, result: Stack { item: 292, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_purple_wool_stairs", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_139, result: Stack { item: 276, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_red_bed", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_140, result: Stack { item: 1233, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_red_carpet", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_141, result: Stack { item: 594, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_red_cushion", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_142, result: Stack { item: 1184, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_red_harness", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_143, result: Stack { item: 964, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_red_wool", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_144, result: Stack { item: 264, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_red_wool_slab", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_145, result: Stack { item: 296, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_red_wool_stairs", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_146, result: Stack { item: 280, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_white_bed", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_147, result: Stack { item: 1219, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_white_carpet", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_148, result: Stack { item: 580, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_white_cushion", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_149, result: Stack { item: 1170, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_white_harness", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_150, result: Stack { item: 950, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_white_wool", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_151, result: Stack { item: 250, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_white_wool_slab", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_152, result: Stack { item: 282, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_white_wool_stairs", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_153, result: Stack { item: 266, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_yellow_bed", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_154, result: Stack { item: 1223, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_yellow_carpet", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_155, result: Stack { item: 584, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_yellow_cushion", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_156, result: Stack { item: 1174, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_yellow_harness", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_157, result: Stack { item: 954, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_yellow_wool", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_158, result: Stack { item: 254, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_yellow_wool_slab", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_159, result: Stack { item: 286, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:dye_yellow_wool_stairs", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_160, result: Stack { item: 270, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:emerald", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_161, result: Stack { item: 1013, count: 9, components: &[0, 0] } } },
    Recipe { id: "minecraft:emerald_block", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_259, result: Stack { item: 514, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:emerald_from_blasting_deepslate_emerald_ore", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_363, result: Stack { item: 1013, count: 1, components: &[0, 0] }, experience: 1.0, time: 200 } },
    Recipe { id: "minecraft:emerald_from_blasting_emerald_ore", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_364, result: Stack { item: 1013, count: 1, components: &[0, 0] }, experience: 1.0, time: 200 } },
    Recipe { id: "minecraft:emerald_from_smelting_deepslate_emerald_ore", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_363, result: Stack { item: 1013, count: 1, components: &[0, 0] }, experience: 1.0, time: 200 } },
    Recipe { id: "minecraft:emerald_from_smelting_emerald_ore", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_364, result: Stack { item: 1013, count: 1, components: &[0, 0] }, experience: 1.0, time: 200 } },
    Recipe { id: "minecraft:enchanting_table", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_260, result: Stack { item: 507, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:end_crystal", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_261, result: Stack { item: 1433, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:end_rod", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_262, result: Stack { item: 396, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:end_stone_brick_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_263, result: Stack { item: 811, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:end_stone_brick_slab_from_end_stone_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_367, result: Stack { item: 811, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:end_stone_brick_slab_from_end_stone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_368, result: Stack { item: 811, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:end_stone_brick_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_264, result: Stack { item: 793, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:end_stone_brick_stairs_from_end_stone_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_367, result: Stack { item: 793, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:end_stone_brick_stairs_from_end_stone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_368, result: Stack { item: 793, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:end_stone_brick_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_265, result: Stack { item: 544, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:end_stone_brick_wall_from_end_stone_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_367, result: Stack { item: 544, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:end_stone_brick_wall_from_end_stone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_368, result: Stack { item: 544, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:end_stone_bricks", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_266, result: Stack { item: 510, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:end_stone_bricks_from_end_stone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_368, result: Stack { item: 510, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:ender_chest", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_267, result: Stack { item: 513, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:ender_eye", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_162, result: Stack { item: 1278, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:exposed_chiseled_copper", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_268, result: Stack { item: 132, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:exposed_chiseled_copper_from_exposed_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_371, result: Stack { item: 132, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:exposed_chiseled_copper_from_exposed_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_372, result: Stack { item: 132, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:exposed_copper_bulb", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_269, result: Stack { item: 1630, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:exposed_copper_grate", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_270, result: Stack { item: 1622, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:exposed_copper_grate_from_exposed_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_371, result: Stack { item: 1622, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:exposed_cut_copper", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_271, result: Stack { item: 140, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:exposed_cut_copper_from_exposed_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_371, result: Stack { item: 140, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:exposed_cut_copper_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_272, result: Stack { item: 156, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:exposed_cut_copper_slab_from_exposed_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_371, result: Stack { item: 156, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:exposed_cut_copper_slab_from_exposed_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_372, result: Stack { item: 156, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:exposed_cut_copper_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_273, result: Stack { item: 148, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:exposed_cut_copper_stairs_from_exposed_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_371, result: Stack { item: 148, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:exposed_cut_copper_stairs_from_exposed_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_372, result: Stack { item: 148, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:eye_armor_trim_smithing_template", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_274, result: Stack { item: 1585, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:eye_armor_trim_smithing_template_smithing_trim", kind: RecipeKind::SmithingTrim { template: INGREDIENT_373, base: INGREDIENT_73, addition: INGREDIENT_74, pattern: "minecraft:eye" } },
    Recipe { id: "minecraft:fermented_spider_eye", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_163, result: Stack { item: 1273, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:field_masoned_banner_pattern", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_164, result: Stack { item: 1502, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:fire_charge", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_165, result: Stack { item: 1369, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:firework_rocket", kind: RecipeKind::FireworkRocket { shell: INGREDIENT_78, fuel: INGREDIENT_106, star: INGREDIENT_375, result: Stack { item: 1393, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:firework_rocket_simple", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_166, result: Stack { item: 1393, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:firework_star", kind: RecipeKind::FireworkStar { shapes: SHAPES_0, trail: INGREDIENT_70, twinkle: INGREDIENT_102, fuel: INGREDIENT_106, dye: INGREDIENT_379, result: Stack { item: 1394, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:firework_star_fade", kind: RecipeKind::FireworkStarFade { target: INGREDIENT_375, dye: INGREDIENT_379, result: Stack { item: 1394, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:fishing_rod", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_275, result: Stack { item: 1186, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:fletching_table", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_276, result: Stack { item: 1510, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:flint_and_steel", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_167, result: Stack { item: 1005, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:flow_armor_trim_smithing_template", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_277, result: Stack { item: 1596, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:flow_armor_trim_smithing_template_smithing_trim", kind: RecipeKind::SmithingTrim { template: INGREDIENT_380, base: INGREDIENT_73, addition: INGREDIENT_74, pattern: "minecraft:flow" } },
    Recipe { id: "minecraft:flower_banner_pattern", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_168, result: Stack { item: 1494, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:flower_pot", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_278, result: Stack { item: 1377, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:furnace", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_279, result: Stack { item: 407, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:furnace_minecart", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_169, result: Stack { item: 968, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:glass", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_382, result: Stack { item: 231, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:glass_bottle", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_280, result: Stack { item: 1270, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:glass_pane", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_281, result: Stack { item: 482, count: 16, components: &[0, 0] } } },
    Recipe { id: "minecraft:glistering_melon_slice", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_282, result: Stack { item: 1279, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:glow_item_frame", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_170, result: Stack { item: 1376, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:glowstone", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_283, result: Stack { item: 441, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:gold_block", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_284, result: Stack { item: 128, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:gold_ingot_from_blasting_deepslate_gold_ore", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_386, result: Stack { item: 1022, count: 1, components: &[0, 0] }, experience: 1.0, time: 200 } },
    Recipe { id: "minecraft:gold_ingot_from_blasting_gold_ore", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_387, result: Stack { item: 1022, count: 1, components: &[0, 0] }, experience: 1.0, time: 200 } },
    Recipe { id: "minecraft:gold_ingot_from_blasting_nether_gold_ore", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_388, result: Stack { item: 1022, count: 1, components: &[0, 0] }, experience: 1.0, time: 200 } },
    Recipe { id: "minecraft:gold_ingot_from_blasting_raw_gold", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_389, result: Stack { item: 1022, count: 1, components: &[0, 0] }, experience: 1.0, time: 200 } },
    Recipe { id: "minecraft:gold_ingot_from_gold_block", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_171, result: Stack { item: 1022, count: 9, components: &[0, 0] } } },
    Recipe { id: "minecraft:gold_ingot_from_nuggets", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_285, result: Stack { item: 1022, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:gold_ingot_from_smelting_deepslate_gold_ore", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_386, result: Stack { item: 1022, count: 1, components: &[0, 0] }, experience: 1.0, time: 200 } },
    Recipe { id: "minecraft:gold_ingot_from_smelting_gold_ore", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_387, result: Stack { item: 1022, count: 1, components: &[0, 0] }, experience: 1.0, time: 200 } },
    Recipe { id: "minecraft:gold_ingot_from_smelting_nether_gold_ore", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_388, result: Stack { item: 1022, count: 1, components: &[0, 0] }, experience: 1.0, time: 200 } },
    Recipe { id: "minecraft:gold_ingot_from_smelting_raw_gold", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_389, result: Stack { item: 1022, count: 1, components: &[0, 0] }, experience: 1.0, time: 200 } },
    Recipe { id: "minecraft:gold_nugget", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_172, result: Stack { item: 1268, count: 9, components: &[0, 0] } } },
    Recipe { id: "minecraft:gold_nugget_from_blasting", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_391, result: Stack { item: 1268, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:gold_nugget_from_smelting", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_391, result: Stack { item: 1268, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:golden_apple", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_286, result: Stack { item: 1100, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:golden_axe", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_287, result: Stack { item: 1043, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:golden_boots", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_288, result: Stack { item: 1091, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:golden_carrot", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_289, result: Stack { item: 1383, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:golden_chestplate", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_290, result: Stack { item: 1089, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:golden_dandelion", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_291, result: Stack { item: 299, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:golden_helmet", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_292, result: Stack { item: 1088, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:golden_hoe", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_293, result: Stack { item: 1044, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:golden_leggings", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_294, result: Stack { item: 1090, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:golden_pickaxe", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_295, result: Stack { item: 1042, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:golden_shovel", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_296, result: Stack { item: 1041, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:golden_spear", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_297, result: Stack { item: 1451, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:golden_sword", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_298, result: Stack { item: 1040, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:granite", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_173, result: Stack { item: 2, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:granite_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_299, result: Stack { item: 814, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:granite_slab_from_granite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_395, result: Stack { item: 814, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:granite_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_300, result: Stack { item: 797, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:granite_stairs_from_granite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_395, result: Stack { item: 797, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:granite_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_301, result: Stack { item: 537, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:granite_wall_from_granite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_395, result: Stack { item: 537, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:gray_banner", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_302, result: Stack { item: 1424, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:gray_banner_duplicate", kind: RecipeKind::BannerDuplicate { banner: INGREDIENT_397, result: Stack { item: 1424, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:gray_bed", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_303, result: Stack { item: 1226, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:gray_bundle", kind: RecipeKind::Transmute { input: INGREDIENT_41, material: INGREDIENT_266, min_material: 1, max_material: 1, result_item: Some(1161), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:gray_candle", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_174, result: Stack { item: 1558, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:gray_carpet", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_304, result: Stack { item: 587, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:gray_concrete_powder", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_175, result: Stack { item: 744, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:gray_concrete_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_305, result: Stack { item: 728, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:gray_concrete_slab_from_gray_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_398, result: Stack { item: 728, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:gray_concrete_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_306, result: Stack { item: 712, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:gray_concrete_stairs_from_gray_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_398, result: Stack { item: 712, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:gray_cushion", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_307, result: Stack { item: 1177, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:gray_dye", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_176, result: Stack { item: 1206, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:gray_dye_from_closed_eyeblossom", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_177, result: Stack { item: 1206, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:gray_glazed_terracotta", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_401, result: Stack { item: 680, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:gray_harness", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_308, result: Stack { item: 957, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:gray_shulker_box", kind: RecipeKind::Transmute { input: INGREDIENT_52, material: INGREDIENT_266, min_material: 1, max_material: 1, result_item: Some(664), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:gray_stained_glass", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_309, result: Stack { item: 612, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:gray_stained_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_310, result: Stack { item: 628, count: 16, components: &[0, 0] } } },
    Recipe { id: "minecraft:gray_stained_glass_pane_from_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_311, result: Stack { item: 628, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:gray_terracotta", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_312, result: Stack { item: 568, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:gray_wool_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_313, result: Stack { item: 289, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:gray_wool_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_314, result: Stack { item: 273, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:green_banner", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_315, result: Stack { item: 1430, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:green_banner_duplicate", kind: RecipeKind::BannerDuplicate { banner: INGREDIENT_404, result: Stack { item: 1430, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:green_bed", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_316, result: Stack { item: 1232, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:green_bundle", kind: RecipeKind::Transmute { input: INGREDIENT_41, material: INGREDIENT_213, min_material: 1, max_material: 1, result_item: Some(1167), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:green_candle", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_178, result: Stack { item: 1564, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:green_carpet", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_317, result: Stack { item: 593, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:green_concrete_powder", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_179, result: Stack { item: 750, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:green_concrete_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_318, result: Stack { item: 734, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:green_concrete_slab_from_green_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_405, result: Stack { item: 734, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:green_concrete_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_319, result: Stack { item: 718, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:green_concrete_stairs_from_green_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_405, result: Stack { item: 718, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:green_cushion", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_320, result: Stack { item: 1183, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:green_dye", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_407, result: Stack { item: 1212, count: 1, components: &[0, 0] }, experience: 1.0, time: 200 } },
    Recipe { id: "minecraft:green_glazed_terracotta", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_408, result: Stack { item: 686, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:green_harness", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_321, result: Stack { item: 963, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:green_shulker_box", kind: RecipeKind::Transmute { input: INGREDIENT_52, material: INGREDIENT_213, min_material: 1, max_material: 1, result_item: Some(670), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:green_stained_glass", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_322, result: Stack { item: 618, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:green_stained_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_323, result: Stack { item: 634, count: 16, components: &[0, 0] } } },
    Recipe { id: "minecraft:green_stained_glass_pane_from_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_324, result: Stack { item: 634, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:green_terracotta", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_325, result: Stack { item: 574, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:green_wool_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_326, result: Stack { item: 295, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:green_wool_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_327, result: Stack { item: 279, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:grindstone", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_328, result: Stack { item: 1511, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:hay_block", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_180, result: Stack { item: 579, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:heavy_weighted_pressure_plate", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_329, result: Stack { item: 874, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:honey_block", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_330, result: Stack { item: 832, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:honey_bottle", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_181, result: Stack { item: 1533, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:honeycomb_block", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_331, result: Stack { item: 1534, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:hopper", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_332, result: Stack { item: 834, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:hopper_minecart", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_182, result: Stack { item: 970, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:host_armor_trim_smithing_template", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_333, result: Stack { item: 1595, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:host_armor_trim_smithing_template_smithing_trim", kind: RecipeKind::SmithingTrim { template: INGREDIENT_415, base: INGREDIENT_73, addition: INGREDIENT_74, pattern: "minecraft:host" } },
    Recipe { id: "minecraft:iron_axe", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_334, result: Stack { item: 1048, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:iron_bars", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_335, result: Stack { item: 464, count: 16, components: &[0, 0] } } },
    Recipe { id: "minecraft:iron_block", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_336, result: Stack { item: 119, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:iron_boots", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_337, result: Stack { item: 1083, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:iron_chain", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_338, result: Stack { item: 473, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:iron_chestplate", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_339, result: Stack { item: 1081, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:iron_door", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_335, result: Stack { item: 888, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:iron_helmet", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_340, result: Stack { item: 1080, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:iron_hoe", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_341, result: Stack { item: 1049, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:iron_ingot_from_blasting_deepslate_iron_ore", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_418, result: Stack { item: 1018, count: 1, components: &[0, 0] }, experience: 0.7, time: 200 } },
    Recipe { id: "minecraft:iron_ingot_from_blasting_iron_ore", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_419, result: Stack { item: 1018, count: 1, components: &[0, 0] }, experience: 0.7, time: 200 } },
    Recipe { id: "minecraft:iron_ingot_from_blasting_raw_iron", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_420, result: Stack { item: 1018, count: 1, components: &[0, 0] }, experience: 0.7, time: 200 } },
    Recipe { id: "minecraft:iron_ingot_from_iron_block", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_183, result: Stack { item: 1018, count: 9, components: &[0, 0] } } },
    Recipe { id: "minecraft:iron_ingot_from_nuggets", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_342, result: Stack { item: 1018, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:iron_ingot_from_smelting_deepslate_iron_ore", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_418, result: Stack { item: 1018, count: 1, components: &[0, 0] }, experience: 0.7, time: 200 } },
    Recipe { id: "minecraft:iron_ingot_from_smelting_iron_ore", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_419, result: Stack { item: 1018, count: 1, components: &[0, 0] }, experience: 0.7, time: 200 } },
    Recipe { id: "minecraft:iron_ingot_from_smelting_raw_iron", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_420, result: Stack { item: 1018, count: 1, components: &[0, 0] }, experience: 0.7, time: 200 } },
    Recipe { id: "minecraft:iron_leggings", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_343, result: Stack { item: 1082, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:iron_nugget", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_184, result: Stack { item: 1456, count: 9, components: &[0, 0] } } },
    Recipe { id: "minecraft:iron_nugget_from_blasting", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_421, result: Stack { item: 1456, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:iron_nugget_from_smelting", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_421, result: Stack { item: 1456, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:iron_pickaxe", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_344, result: Stack { item: 1047, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:iron_shovel", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_345, result: Stack { item: 1046, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:iron_spear", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_346, result: Stack { item: 1450, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:iron_sword", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_347, result: Stack { item: 1045, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:iron_trapdoor", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_348, result: Stack { item: 910, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:item_frame", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_349, result: Stack { item: 1375, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:jack_o_lantern", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_350, result: Stack { item: 432, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:jukebox", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_351, result: Stack { item: 416, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:jungle_boat", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_352, result: Stack { item: 981, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:jungle_button", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_185, result: Stack { item: 861, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:jungle_chest_boat", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_186, result: Stack { item: 982, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:jungle_door", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_353, result: Stack { item: 892, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:jungle_fence", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_354, result: Stack { item: 420, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:jungle_fence_gate", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_355, result: Stack { item: 935, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:jungle_hanging_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_356, result: Stack { item: 1118, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:jungle_planks", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_187, result: Stack { item: 66, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:jungle_pressure_plate", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_357, result: Stack { item: 878, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:jungle_shelf", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_358, result: Stack { item: 383, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:jungle_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_359, result: Stack { item: 1105, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:jungle_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_360, result: Stack { item: 344, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:jungle_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_361, result: Stack { item: 518, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:jungle_trapdoor", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_353, result: Stack { item: 914, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:jungle_wood", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_362, result: Stack { item: 206, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:ladder", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_363, result: Stack { item: 408, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:lantern", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_364, result: Stack { item: 1515, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:lapis_block", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_365, result: Stack { item: 233, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:lapis_lazuli", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_188, result: Stack { item: 1014, count: 9, components: &[0, 0] } } },
    Recipe { id: "minecraft:lapis_lazuli_from_blasting_deepslate_lapis_ore", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_430, result: Stack { item: 1014, count: 1, components: &[0, 0] }, experience: 0.2, time: 200 } },
    Recipe { id: "minecraft:lapis_lazuli_from_blasting_lapis_ore", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_431, result: Stack { item: 1014, count: 1, components: &[0, 0] }, experience: 0.2, time: 200 } },
    Recipe { id: "minecraft:lapis_lazuli_from_smelting_deepslate_lapis_ore", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_430, result: Stack { item: 1014, count: 1, components: &[0, 0] }, experience: 0.2, time: 200 } },
    Recipe { id: "minecraft:lapis_lazuli_from_smelting_lapis_ore", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_431, result: Stack { item: 1014, count: 1, components: &[0, 0] }, experience: 0.2, time: 200 } },
    Recipe { id: "minecraft:lead", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_366, result: Stack { item: 1412, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:leaf_litter", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_432, result: Stack { item: 331, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:leather", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_367, result: Stack { item: 1133, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:leather_boots", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_368, result: Stack { item: 1071, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:leather_boots_dyed", kind: RecipeKind::Dye { target: INGREDIENT_434, dye: INGREDIENT_379, result: Stack { item: 1071, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:leather_chestplate", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_369, result: Stack { item: 1069, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:leather_chestplate_dyed", kind: RecipeKind::Dye { target: INGREDIENT_435, dye: INGREDIENT_379, result: Stack { item: 1069, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:leather_helmet", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_370, result: Stack { item: 1068, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:leather_helmet_dyed", kind: RecipeKind::Dye { target: INGREDIENT_436, dye: INGREDIENT_379, result: Stack { item: 1068, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:leather_horse_armor", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_371, result: Stack { item: 1411, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:leather_horse_armor_dyed", kind: RecipeKind::Dye { target: INGREDIENT_437, dye: INGREDIENT_379, result: Stack { item: 1411, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:leather_leggings", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_372, result: Stack { item: 1070, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:leather_leggings_dyed", kind: RecipeKind::Dye { target: INGREDIENT_438, dye: INGREDIENT_379, result: Stack { item: 1070, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:lectern", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_373, result: Stack { item: 837, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:lever", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_374, result: Stack { item: 839, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_blue_banner", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_375, result: Stack { item: 1420, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_blue_banner_duplicate", kind: RecipeKind::BannerDuplicate { banner: INGREDIENT_441, result: Stack { item: 1420, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_blue_bed", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_376, result: Stack { item: 1222, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_blue_bundle", kind: RecipeKind::Transmute { input: INGREDIENT_41, material: INGREDIENT_281, min_material: 1, max_material: 1, result_item: Some(1157), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:light_blue_candle", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_189, result: Stack { item: 1554, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_blue_carpet", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_377, result: Stack { item: 583, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_blue_concrete_powder", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_190, result: Stack { item: 740, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_blue_concrete_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_378, result: Stack { item: 724, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_blue_concrete_slab_from_light_blue_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_442, result: Stack { item: 724, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_blue_concrete_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_379, result: Stack { item: 708, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_blue_concrete_stairs_from_light_blue_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_442, result: Stack { item: 708, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_blue_cushion", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_380, result: Stack { item: 1173, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_blue_dye_from_blue_orchid", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_191, result: Stack { item: 1202, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_blue_dye_from_blue_white_dye", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_192, result: Stack { item: 1202, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_blue_glazed_terracotta", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_445, result: Stack { item: 676, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:light_blue_harness", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_381, result: Stack { item: 953, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_blue_shulker_box", kind: RecipeKind::Transmute { input: INGREDIENT_52, material: INGREDIENT_281, min_material: 1, max_material: 1, result_item: Some(660), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:light_blue_stained_glass", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_382, result: Stack { item: 608, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_blue_stained_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_383, result: Stack { item: 624, count: 16, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_blue_stained_glass_pane_from_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_384, result: Stack { item: 624, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_blue_terracotta", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_385, result: Stack { item: 564, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_blue_wool_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_386, result: Stack { item: 285, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_blue_wool_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_387, result: Stack { item: 269, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_banner", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_388, result: Stack { item: 1425, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_banner_duplicate", kind: RecipeKind::BannerDuplicate { banner: INGREDIENT_448, result: Stack { item: 1425, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_bed", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_389, result: Stack { item: 1227, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_bundle", kind: RecipeKind::Transmute { input: INGREDIENT_41, material: INGREDIENT_289, min_material: 1, max_material: 1, result_item: Some(1162), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:light_gray_candle", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_193, result: Stack { item: 1559, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_carpet", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_390, result: Stack { item: 588, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_concrete_powder", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_194, result: Stack { item: 745, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_concrete_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_391, result: Stack { item: 729, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_concrete_slab_from_light_gray_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_449, result: Stack { item: 729, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_concrete_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_392, result: Stack { item: 713, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_concrete_stairs_from_light_gray_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_449, result: Stack { item: 713, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_cushion", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_393, result: Stack { item: 1178, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_dye_from_azure_bluet", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_195, result: Stack { item: 1207, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_dye_from_black_white_dye", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_196, result: Stack { item: 1207, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_dye_from_gray_white_dye", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_197, result: Stack { item: 1207, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_dye_from_oxeye_daisy", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_198, result: Stack { item: 1207, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_dye_from_white_tulip", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_199, result: Stack { item: 1207, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_glazed_terracotta", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_453, result: Stack { item: 681, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:light_gray_harness", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_394, result: Stack { item: 958, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_shulker_box", kind: RecipeKind::Transmute { input: INGREDIENT_52, material: INGREDIENT_289, min_material: 1, max_material: 1, result_item: Some(665), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:light_gray_stained_glass", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_395, result: Stack { item: 613, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_stained_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_396, result: Stack { item: 629, count: 16, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_stained_glass_pane_from_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_397, result: Stack { item: 629, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_terracotta", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_398, result: Stack { item: 569, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_wool_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_399, result: Stack { item: 290, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_gray_wool_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_400, result: Stack { item: 274, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:light_weighted_pressure_plate", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_401, result: Stack { item: 873, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:lightning_rod", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_402, result: Stack { item: 840, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:lime_banner", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_403, result: Stack { item: 1422, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:lime_banner_duplicate", kind: RecipeKind::BannerDuplicate { banner: INGREDIENT_456, result: Stack { item: 1422, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:lime_bed", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_404, result: Stack { item: 1224, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:lime_bundle", kind: RecipeKind::Transmute { input: INGREDIENT_41, material: INGREDIENT_297, min_material: 1, max_material: 1, result_item: Some(1159), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:lime_candle", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_200, result: Stack { item: 1556, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:lime_carpet", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_405, result: Stack { item: 585, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:lime_concrete_powder", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_201, result: Stack { item: 742, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:lime_concrete_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_406, result: Stack { item: 726, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:lime_concrete_slab_from_lime_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_457, result: Stack { item: 726, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:lime_concrete_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_407, result: Stack { item: 710, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:lime_concrete_stairs_from_lime_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_457, result: Stack { item: 710, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:lime_cushion", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_408, result: Stack { item: 1175, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:lime_dye", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_202, result: Stack { item: 1204, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:lime_dye_from_smelting", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_459, result: Stack { item: 1204, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:lime_glazed_terracotta", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_460, result: Stack { item: 678, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:lime_harness", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_409, result: Stack { item: 955, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:lime_shulker_box", kind: RecipeKind::Transmute { input: INGREDIENT_52, material: INGREDIENT_297, min_material: 1, max_material: 1, result_item: Some(662), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:lime_stained_glass", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_410, result: Stack { item: 610, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:lime_stained_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_411, result: Stack { item: 626, count: 16, components: &[0, 0] } } },
    Recipe { id: "minecraft:lime_stained_glass_pane_from_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_412, result: Stack { item: 626, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:lime_terracotta", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_413, result: Stack { item: 566, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:lime_wool_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_414, result: Stack { item: 287, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:lime_wool_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_415, result: Stack { item: 271, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:lodestone", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_416, result: Stack { item: 1535, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:loom", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_417, result: Stack { item: 1493, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:mace", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_418, result: Stack { item: 1374, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_banner", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_419, result: Stack { item: 1419, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_banner_duplicate", kind: RecipeKind::BannerDuplicate { banner: INGREDIENT_465, result: Stack { item: 1419, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_bed", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_420, result: Stack { item: 1221, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_bundle", kind: RecipeKind::Transmute { input: INGREDIENT_41, material: INGREDIENT_305, min_material: 1, max_material: 1, result_item: Some(1156), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:magenta_candle", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_203, result: Stack { item: 1553, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_carpet", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_421, result: Stack { item: 582, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_concrete_powder", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_204, result: Stack { item: 739, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_concrete_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_422, result: Stack { item: 723, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_concrete_slab_from_magenta_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_466, result: Stack { item: 723, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_concrete_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_423, result: Stack { item: 707, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_concrete_stairs_from_magenta_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_466, result: Stack { item: 707, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_cushion", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_424, result: Stack { item: 1172, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_dye_from_allium", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_205, result: Stack { item: 1201, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_dye_from_blue_red_pink", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_206, result: Stack { item: 1201, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_dye_from_blue_red_white_dye", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_207, result: Stack { item: 1201, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_dye_from_lilac", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_208, result: Stack { item: 1201, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_dye_from_purple_and_pink", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_209, result: Stack { item: 1201, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_glazed_terracotta", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_470, result: Stack { item: 675, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:magenta_harness", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_425, result: Stack { item: 952, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_shulker_box", kind: RecipeKind::Transmute { input: INGREDIENT_52, material: INGREDIENT_305, min_material: 1, max_material: 1, result_item: Some(659), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:magenta_stained_glass", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_426, result: Stack { item: 607, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_stained_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_427, result: Stack { item: 623, count: 16, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_stained_glass_pane_from_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_428, result: Stack { item: 623, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_terracotta", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_429, result: Stack { item: 563, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_wool_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_430, result: Stack { item: 284, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:magenta_wool_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_431, result: Stack { item: 268, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:magma_block", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_432, result: Stack { item: 650, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:magma_cream", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_210, result: Stack { item: 1275, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:mangrove_boat", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_433, result: Stack { item: 991, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:mangrove_button", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_211, result: Stack { item: 866, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:mangrove_chest_boat", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_212, result: Stack { item: 992, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:mangrove_door", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_434, result: Stack { item: 897, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:mangrove_fence", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_435, result: Stack { item: 425, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:mangrove_fence_gate", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_436, result: Stack { item: 940, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:mangrove_hanging_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_437, result: Stack { item: 1123, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:mangrove_planks", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_213, result: Stack { item: 71, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:mangrove_pressure_plate", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_438, result: Stack { item: 883, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:mangrove_shelf", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_439, result: Stack { item: 384, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:mangrove_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_440, result: Stack { item: 1110, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:mangrove_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_441, result: Stack { item: 349, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:mangrove_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_442, result: Stack { item: 523, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:mangrove_trapdoor", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_434, result: Stack { item: 919, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:mangrove_wood", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_443, result: Stack { item: 211, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:map", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_444, result: Stack { item: 1382, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:map_cloning", kind: RecipeKind::Transmute { input: INGREDIENT_479, material: INGREDIENT_480, min_material: 1, max_material: 8, result_item: None, result_count: 1, result_components: &[0, 0], add_material_count: true } },
    Recipe { id: "minecraft:map_extending", kind: RecipeKind::MapExtending { map: INGREDIENT_481, material: INGREDIENT_78, result_item: None, result_count: 1, result_components: &[0, 0] } },
    Recipe { id: "minecraft:melon", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_214, result: Stack { item: 483, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:melon_seeds", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_215, result: Stack { item: 1259, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:minecart", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_445, result: Stack { item: 966, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:mojang_banner_pattern", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_216, result: Stack { item: 1497, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:moss_carpet", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_446, result: Stack { item: 332, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:mossy_cobblestone_from_moss_block", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_217, result: Stack { item: 393, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:mossy_cobblestone_from_vine", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_218, result: Stack { item: 393, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:mossy_cobblestone_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_447, result: Stack { item: 810, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:mossy_cobblestone_slab_from_mossy_cobblestone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_484, result: Stack { item: 810, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:mossy_cobblestone_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_448, result: Stack { item: 792, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:mossy_cobblestone_stairs_from_mossy_cobblestone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_484, result: Stack { item: 792, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:mossy_cobblestone_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_449, result: Stack { item: 532, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:mossy_cobblestone_wall_from_mossy_cobblestone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_484, result: Stack { item: 532, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:mossy_stone_brick_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_450, result: Stack { item: 808, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:mossy_stone_brick_slab_from_mossy_stone_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_485, result: Stack { item: 808, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:mossy_stone_brick_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_451, result: Stack { item: 790, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:mossy_stone_brick_stairs_from_mossy_stone_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_485, result: Stack { item: 790, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:mossy_stone_brick_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_452, result: Stack { item: 536, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:mossy_stone_brick_wall_from_mossy_stone_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_485, result: Stack { item: 536, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:mossy_stone_bricks_from_moss_block", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_219, result: Stack { item: 450, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:mossy_stone_bricks_from_vine", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_220, result: Stack { item: 450, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:mud_brick_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_453, result: Stack { item: 363, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:mud_brick_slab_from_mud_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_486, result: Stack { item: 363, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:mud_brick_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_454, result: Stack { item: 495, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:mud_brick_stairs_from_mud_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_486, result: Stack { item: 495, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:mud_brick_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_455, result: Stack { item: 539, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:mud_brick_wall_from_mud_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_486, result: Stack { item: 539, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:mud_bricks", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_456, result: Stack { item: 454, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:muddy_mangrove_roots", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_221, result: Stack { item: 174, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:mushroom_stew", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_222, result: Stack { item: 1061, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:music_disc_5", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_223, result: Stack { item: 1478, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:name_tag", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_457, result: Stack { item: 1413, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:nether_brick", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_493, result: Stack { item: 1396, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:nether_brick_fence", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_458, result: Stack { item: 501, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:nether_brick_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_459, result: Stack { item: 364, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:nether_brick_slab_from_nether_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_145, result: Stack { item: 364, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:nether_brick_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_460, result: Stack { item: 502, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:nether_brick_stairs_from_nether_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_145, result: Stack { item: 502, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:nether_brick_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_461, result: Stack { item: 540, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:nether_brick_wall_from_nether_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_145, result: Stack { item: 540, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:nether_bricks", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_462, result: Stack { item: 498, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:nether_wart_block", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_224, result: Stack { item: 651, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:netherite_axe_smithing", kind: RecipeKind::SmithingTransform { template: Some(INGREDIENT_495), base: INGREDIENT_496, addition: Some(INGREDIENT_497), result: Stack { item: 1058, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:netherite_block", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_463, result: Stack { item: 130, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:netherite_boots_smithing", kind: RecipeKind::SmithingTransform { template: Some(INGREDIENT_495), base: INGREDIENT_499, addition: Some(INGREDIENT_497), result: Stack { item: 1095, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:netherite_chestplate_smithing", kind: RecipeKind::SmithingTransform { template: Some(INGREDIENT_495), base: INGREDIENT_500, addition: Some(INGREDIENT_497), result: Stack { item: 1093, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:netherite_helmet_smithing", kind: RecipeKind::SmithingTransform { template: Some(INGREDIENT_495), base: INGREDIENT_501, addition: Some(INGREDIENT_497), result: Stack { item: 1092, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:netherite_hoe_smithing", kind: RecipeKind::SmithingTransform { template: Some(INGREDIENT_495), base: INGREDIENT_502, addition: Some(INGREDIENT_497), result: Stack { item: 1059, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:netherite_horse_armor_smithing", kind: RecipeKind::SmithingTransform { template: Some(INGREDIENT_495), base: INGREDIENT_503, addition: Some(INGREDIENT_497), result: Stack { item: 1410, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:netherite_ingot", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_225, result: Stack { item: 1023, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:netherite_ingot_from_netherite_block", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_226, result: Stack { item: 1023, count: 9, components: &[0, 0] } } },
    Recipe { id: "minecraft:netherite_leggings_smithing", kind: RecipeKind::SmithingTransform { template: Some(INGREDIENT_495), base: INGREDIENT_506, addition: Some(INGREDIENT_497), result: Stack { item: 1094, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:netherite_nautilus_armor_smithing", kind: RecipeKind::SmithingTransform { template: Some(INGREDIENT_495), base: INGREDIENT_507, addition: Some(INGREDIENT_497), result: Stack { item: 1488, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:netherite_pickaxe_smithing", kind: RecipeKind::SmithingTransform { template: Some(INGREDIENT_495), base: INGREDIENT_508, addition: Some(INGREDIENT_497), result: Stack { item: 1057, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:netherite_scrap", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_509, result: Stack { item: 1024, count: 1, components: &[0, 0] }, experience: 2.0, time: 200 } },
    Recipe { id: "minecraft:netherite_scrap_from_blasting", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_509, result: Stack { item: 1024, count: 1, components: &[0, 0] }, experience: 2.0, time: 200 } },
    Recipe { id: "minecraft:netherite_shovel_smithing", kind: RecipeKind::SmithingTransform { template: Some(INGREDIENT_495), base: INGREDIENT_510, addition: Some(INGREDIENT_497), result: Stack { item: 1056, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:netherite_spear_smithing", kind: RecipeKind::SmithingTransform { template: Some(INGREDIENT_495), base: INGREDIENT_511, addition: Some(INGREDIENT_497), result: Stack { item: 1453, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:netherite_sword_smithing", kind: RecipeKind::SmithingTransform { template: Some(INGREDIENT_495), base: INGREDIENT_512, addition: Some(INGREDIENT_497), result: Stack { item: 1055, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:netherite_upgrade_smithing_template", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_464, result: Stack { item: 1579, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:note_block", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_465, result: Stack { item: 855, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:oak_boat", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_466, result: Stack { item: 975, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:oak_button", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_227, result: Stack { item: 858, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:oak_chest_boat", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_228, result: Stack { item: 976, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:oak_door", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_467, result: Stack { item: 889, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:oak_fence", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_468, result: Stack { item: 417, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:oak_fence_gate", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_469, result: Stack { item: 932, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:oak_hanging_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_470, result: Stack { item: 1115, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:oak_planks", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_229, result: Stack { item: 63, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:oak_pressure_plate", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_471, result: Stack { item: 875, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:oak_shelf", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_472, result: Stack { item: 386, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:oak_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_473, result: Stack { item: 1102, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:oak_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_474, result: Stack { item: 341, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:oak_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_475, result: Stack { item: 515, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:oak_trapdoor", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_467, result: Stack { item: 911, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:oak_wood", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_476, result: Stack { item: 203, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:observer", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_477, result: Stack { item: 833, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:orange_banner", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_478, result: Stack { item: 1418, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:orange_banner_duplicate", kind: RecipeKind::BannerDuplicate { banner: INGREDIENT_519, result: Stack { item: 1418, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:orange_bed", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_479, result: Stack { item: 1220, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:orange_bundle", kind: RecipeKind::Transmute { input: INGREDIENT_41, material: INGREDIENT_313, min_material: 1, max_material: 1, result_item: Some(1155), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:orange_candle", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_230, result: Stack { item: 1552, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:orange_carpet", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_480, result: Stack { item: 581, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:orange_concrete_powder", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_231, result: Stack { item: 738, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:orange_concrete_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_481, result: Stack { item: 722, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:orange_concrete_slab_from_orange_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_520, result: Stack { item: 722, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:orange_concrete_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_482, result: Stack { item: 706, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:orange_concrete_stairs_from_orange_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_520, result: Stack { item: 706, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:orange_cushion", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_483, result: Stack { item: 1171, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:orange_dye_from_open_eyeblossom", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_232, result: Stack { item: 1200, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:orange_dye_from_orange_tulip", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_233, result: Stack { item: 1200, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:orange_dye_from_red_yellow", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_234, result: Stack { item: 1200, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:orange_dye_from_torchflower", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_235, result: Stack { item: 1200, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:orange_glazed_terracotta", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_525, result: Stack { item: 674, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:orange_harness", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_484, result: Stack { item: 951, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:orange_shulker_box", kind: RecipeKind::Transmute { input: INGREDIENT_52, material: INGREDIENT_313, min_material: 1, max_material: 1, result_item: Some(658), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:orange_stained_glass", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_485, result: Stack { item: 606, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:orange_stained_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_486, result: Stack { item: 622, count: 16, components: &[0, 0] } } },
    Recipe { id: "minecraft:orange_stained_glass_pane_from_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_487, result: Stack { item: 622, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:orange_terracotta", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_488, result: Stack { item: 562, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:orange_wool_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_489, result: Stack { item: 283, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:orange_wool_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_490, result: Stack { item: 267, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:oxidized_chiseled_copper", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_491, result: Stack { item: 134, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:oxidized_chiseled_copper_from_oxidized_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_528, result: Stack { item: 134, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:oxidized_chiseled_copper_from_oxidized_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_529, result: Stack { item: 134, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:oxidized_copper_bulb", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_492, result: Stack { item: 1632, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:oxidized_copper_grate", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_493, result: Stack { item: 1624, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:oxidized_copper_grate_from_oxidized_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_528, result: Stack { item: 1624, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:oxidized_cut_copper", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_494, result: Stack { item: 142, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:oxidized_cut_copper_from_oxidized_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_528, result: Stack { item: 142, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:oxidized_cut_copper_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_495, result: Stack { item: 158, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:oxidized_cut_copper_slab_from_oxidized_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_528, result: Stack { item: 158, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:oxidized_cut_copper_slab_from_oxidized_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_529, result: Stack { item: 158, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:oxidized_cut_copper_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_496, result: Stack { item: 150, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:oxidized_cut_copper_stairs_from_oxidized_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_528, result: Stack { item: 150, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:oxidized_cut_copper_stairs_from_oxidized_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_529, result: Stack { item: 150, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:packed_ice", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_236, result: Stack { item: 597, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:packed_mud", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_237, result: Stack { item: 453, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:painting", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_497, result: Stack { item: 1099, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:pale_moss_carpet", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_498, result: Stack { item: 334, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:pale_oak_boat", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_499, result: Stack { item: 989, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:pale_oak_button", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_238, result: Stack { item: 865, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:pale_oak_chest_boat", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_239, result: Stack { item: 990, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:pale_oak_door", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_500, result: Stack { item: 896, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:pale_oak_fence", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_501, result: Stack { item: 424, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:pale_oak_fence_gate", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_502, result: Stack { item: 939, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:pale_oak_hanging_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_503, result: Stack { item: 1122, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:pale_oak_planks", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_240, result: Stack { item: 70, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:pale_oak_pressure_plate", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_504, result: Stack { item: 882, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:pale_oak_shelf", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_505, result: Stack { item: 387, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:pale_oak_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_506, result: Stack { item: 1109, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:pale_oak_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_507, result: Stack { item: 348, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:pale_oak_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_508, result: Stack { item: 522, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:pale_oak_trapdoor", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_500, result: Stack { item: 918, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:pale_oak_wood", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_509, result: Stack { item: 209, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:paper", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_510, result: Stack { item: 1145, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_banner", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_511, result: Stack { item: 1423, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_banner_duplicate", kind: RecipeKind::BannerDuplicate { banner: INGREDIENT_539, result: Stack { item: 1423, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_bed", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_512, result: Stack { item: 1225, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_bundle", kind: RecipeKind::Transmute { input: INGREDIENT_41, material: INGREDIENT_321, min_material: 1, max_material: 1, result_item: Some(1160), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:pink_candle", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_241, result: Stack { item: 1557, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_carpet", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_513, result: Stack { item: 586, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_concrete_powder", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_242, result: Stack { item: 743, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_concrete_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_514, result: Stack { item: 727, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_concrete_slab_from_pink_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_540, result: Stack { item: 727, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_concrete_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_515, result: Stack { item: 711, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_concrete_stairs_from_pink_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_540, result: Stack { item: 711, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_cushion", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_516, result: Stack { item: 1176, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_dye_from_cactus_flower", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_243, result: Stack { item: 1205, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_dye_from_peony", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_244, result: Stack { item: 1205, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_dye_from_pink_petals", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_245, result: Stack { item: 1205, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_dye_from_pink_tulip", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_246, result: Stack { item: 1205, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_dye_from_red_white_dye", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_247, result: Stack { item: 1205, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_glazed_terracotta", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_546, result: Stack { item: 679, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:pink_harness", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_517, result: Stack { item: 956, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_shulker_box", kind: RecipeKind::Transmute { input: INGREDIENT_52, material: INGREDIENT_321, min_material: 1, max_material: 1, result_item: Some(663), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:pink_stained_glass", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_518, result: Stack { item: 611, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_stained_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_519, result: Stack { item: 627, count: 16, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_stained_glass_pane_from_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_520, result: Stack { item: 627, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_terracotta", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_521, result: Stack { item: 567, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_wool_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_522, result: Stack { item: 288, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:pink_wool_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_523, result: Stack { item: 272, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:piston", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_524, result: Stack { item: 829, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_andesite", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_525, result: Stack { item: 7, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_andesite_from_andesite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_13, result: Stack { item: 7, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_andesite_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_526, result: Stack { item: 817, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_andesite_slab_from_andesite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_13, result: Stack { item: 817, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_andesite_slab_from_polished_andesite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_548, result: Stack { item: 817, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_andesite_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_527, result: Stack { item: 800, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_andesite_stairs_from_andesite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_13, result: Stack { item: 800, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_andesite_stairs_from_polished_andesite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_548, result: Stack { item: 800, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_basalt", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_528, result: Stack { item: 437, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_basalt_from_basalt_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_549, result: Stack { item: 437, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_529, result: Stack { item: 1541, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_brick_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_530, result: Stack { item: 1546, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_brick_slab_from_blackstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_56, result: Stack { item: 1546, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_brick_slab_from_polished_blackstone_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_195, result: Stack { item: 1546, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_brick_slab_from_polished_blackstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_147, result: Stack { item: 1546, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_brick_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_531, result: Stack { item: 1547, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_brick_stairs_from_blackstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_56, result: Stack { item: 1547, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_brick_stairs_from_polished_blackstone_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_195, result: Stack { item: 1547, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_brick_stairs_from_polished_blackstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_147, result: Stack { item: 1547, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_brick_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_532, result: Stack { item: 548, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_brick_wall_from_blackstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_56, result: Stack { item: 548, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_brick_wall_from_polished_blackstone_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_195, result: Stack { item: 548, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_brick_wall_from_polished_blackstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_147, result: Stack { item: 548, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_bricks", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_533, result: Stack { item: 1545, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_bricks_from_blackstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_56, result: Stack { item: 1545, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_bricks_from_polished_blackstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_147, result: Stack { item: 1545, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_button", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_248, result: Stack { item: 857, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_from_blackstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_56, result: Stack { item: 1541, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_pressure_plate", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_534, result: Stack { item: 872, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_535, result: Stack { item: 1542, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_slab_from_blackstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_56, result: Stack { item: 1542, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_slab_from_polished_blackstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_147, result: Stack { item: 1542, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_536, result: Stack { item: 1543, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_stairs_from_blackstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_56, result: Stack { item: 1543, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_stairs_from_polished_blackstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_147, result: Stack { item: 1543, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_537, result: Stack { item: 547, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_wall_from_blackstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_56, result: Stack { item: 547, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_blackstone_wall_from_polished_blackstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_147, result: Stack { item: 547, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_cinnabar", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_538, result: Stack { item: 44, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_cinnabar_from_cinnabar_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_137, result: Stack { item: 44, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_cinnabar_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_539, result: Stack { item: 45, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_cinnabar_slab_from_cinnabar_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_137, result: Stack { item: 45, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_cinnabar_slab_from_polished_cinnabar_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_166, result: Stack { item: 45, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_cinnabar_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_540, result: Stack { item: 46, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_cinnabar_stairs_from_cinnabar_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_137, result: Stack { item: 46, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_cinnabar_stairs_from_polished_cinnabar_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_166, result: Stack { item: 46, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_cinnabar_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_541, result: Stack { item: 47, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_cinnabar_wall_from_cinnabar_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_137, result: Stack { item: 47, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_cinnabar_wall_from_polished_cinnabar_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_166, result: Stack { item: 47, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_deepslate", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_542, result: Stack { item: 10, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_deepslate_from_cobbled_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_142, result: Stack { item: 10, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_deepslate_from_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_143, result: Stack { item: 10, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_deepslate_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_543, result: Stack { item: 820, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_deepslate_slab_from_cobbled_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_142, result: Stack { item: 820, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_deepslate_slab_from_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_143, result: Stack { item: 820, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_deepslate_slab_from_polished_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_225, result: Stack { item: 820, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_deepslate_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_544, result: Stack { item: 803, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_deepslate_stairs_from_cobbled_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_142, result: Stack { item: 803, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_deepslate_stairs_from_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_143, result: Stack { item: 803, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_deepslate_stairs_from_polished_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_225, result: Stack { item: 803, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_deepslate_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_545, result: Stack { item: 550, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_deepslate_wall_from_cobbled_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_142, result: Stack { item: 550, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_deepslate_wall_from_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_143, result: Stack { item: 550, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_deepslate_wall_from_polished_deepslate_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_225, result: Stack { item: 550, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_diorite", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_546, result: Stack { item: 5, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_diorite_from_diorite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_11, result: Stack { item: 5, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_diorite_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_547, result: Stack { item: 809, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_diorite_slab_from_diorite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_11, result: Stack { item: 809, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_diorite_slab_from_polished_diorite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_550, result: Stack { item: 809, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_diorite_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_548, result: Stack { item: 791, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_diorite_stairs_from_diorite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_11, result: Stack { item: 791, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_diorite_stairs_from_polished_diorite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_550, result: Stack { item: 791, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_granite", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_549, result: Stack { item: 3, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_granite_from_granite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_395, result: Stack { item: 3, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_granite_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_550, result: Stack { item: 806, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_granite_slab_from_granite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_395, result: Stack { item: 806, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_granite_slab_from_polished_granite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_551, result: Stack { item: 806, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_granite_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_551, result: Stack { item: 788, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_granite_stairs_from_granite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_395, result: Stack { item: 788, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_granite_stairs_from_polished_granite_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_551, result: Stack { item: 788, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_sulfur", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_552, result: Stack { item: 31, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_sulfur_from_sulfur_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_159, result: Stack { item: 31, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_sulfur_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_553, result: Stack { item: 32, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_sulfur_slab_from_polished_sulfur_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_552, result: Stack { item: 32, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_sulfur_slab_from_sulfur_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_159, result: Stack { item: 32, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_sulfur_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_554, result: Stack { item: 33, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_sulfur_stairs_from_polished_sulfur_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_552, result: Stack { item: 33, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_sulfur_stairs_from_sulfur_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_159, result: Stack { item: 33, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_sulfur_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_555, result: Stack { item: 34, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_sulfur_wall_from_polished_sulfur_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_552, result: Stack { item: 34, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_sulfur_wall_from_sulfur_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_159, result: Stack { item: 34, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_tuff", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_556, result: Stack { item: 17, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_tuff_from_tuff_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_164, result: Stack { item: 17, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_tuff_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_557, result: Stack { item: 18, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_tuff_slab_from_polished_tuff_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_162, result: Stack { item: 18, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_tuff_slab_from_tuff_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_164, result: Stack { item: 18, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_tuff_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_558, result: Stack { item: 19, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_tuff_stairs_from_polished_tuff_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_162, result: Stack { item: 19, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_tuff_stairs_from_tuff_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_164, result: Stack { item: 19, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_tuff_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_559, result: Stack { item: 20, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_tuff_wall_from_polished_tuff_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_162, result: Stack { item: 20, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:polished_tuff_wall_from_tuff_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_164, result: Stack { item: 20, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:poplar_boat", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_560, result: Stack { item: 993, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:poplar_button", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_249, result: Stack { item: 867, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:poplar_chest_boat", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_250, result: Stack { item: 994, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:poplar_door", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_561, result: Stack { item: 898, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:poplar_fence", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_562, result: Stack { item: 426, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:poplar_fence_gate", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_563, result: Stack { item: 941, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:poplar_hanging_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_564, result: Stack { item: 1124, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:poplar_planks", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_251, result: Stack { item: 72, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:poplar_pressure_plate", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_565, result: Stack { item: 884, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:poplar_shelf", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_566, result: Stack { item: 385, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:poplar_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_567, result: Stack { item: 1111, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:poplar_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_568, result: Stack { item: 350, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:poplar_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_569, result: Stack { item: 524, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:poplar_trapdoor", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_561, result: Stack { item: 920, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:poplar_wood", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_570, result: Stack { item: 212, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:popped_chorus_fruit", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_558, result: Stack { item: 1435, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:potent_sulfur", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_252, result: Stack { item: 27, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:powered_rail", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_571, result: Stack { item: 945, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:prismarine", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_572, result: Stack { item: 637, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:prismarine_brick_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_573, result: Stack { item: 370, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:prismarine_brick_slab_from_prismarine_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_559, result: Stack { item: 370, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:prismarine_brick_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_574, result: Stack { item: 641, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:prismarine_brick_stairs_from_prismarine_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_559, result: Stack { item: 641, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:prismarine_bricks", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_253, result: Stack { item: 638, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:prismarine_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_575, result: Stack { item: 369, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:prismarine_slab_from_prismarine_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_560, result: Stack { item: 369, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:prismarine_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_576, result: Stack { item: 640, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:prismarine_stairs_from_prismarine_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_560, result: Stack { item: 640, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:prismarine_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_577, result: Stack { item: 534, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:prismarine_wall_from_prismarine_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_560, result: Stack { item: 534, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:pumpkin_pie", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_254, result: Stack { item: 1392, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:pumpkin_seeds", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_255, result: Stack { item: 1258, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:purple_banner", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_578, result: Stack { item: 1427, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:purple_banner_duplicate", kind: RecipeKind::BannerDuplicate { banner: INGREDIENT_563, result: Stack { item: 1427, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:purple_bed", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_579, result: Stack { item: 1229, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:purple_bundle", kind: RecipeKind::Transmute { input: INGREDIENT_41, material: INGREDIENT_329, min_material: 1, max_material: 1, result_item: Some(1164), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:purple_candle", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_256, result: Stack { item: 1561, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:purple_carpet", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_580, result: Stack { item: 590, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:purple_concrete_powder", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_257, result: Stack { item: 747, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:purple_concrete_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_581, result: Stack { item: 731, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:purple_concrete_slab_from_purple_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_564, result: Stack { item: 731, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:purple_concrete_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_582, result: Stack { item: 715, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:purple_concrete_stairs_from_purple_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_564, result: Stack { item: 715, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:purple_cushion", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_583, result: Stack { item: 1180, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:purple_dye", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_258, result: Stack { item: 1209, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:purple_glazed_terracotta", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_566, result: Stack { item: 683, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:purple_harness", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_584, result: Stack { item: 960, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:purple_shulker_box", kind: RecipeKind::Transmute { input: INGREDIENT_52, material: INGREDIENT_329, min_material: 1, max_material: 1, result_item: Some(667), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:purple_stained_glass", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_585, result: Stack { item: 615, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:purple_stained_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_586, result: Stack { item: 631, count: 16, components: &[0, 0] } } },
    Recipe { id: "minecraft:purple_stained_glass_pane_from_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_587, result: Stack { item: 631, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:purple_terracotta", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_588, result: Stack { item: 571, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:purple_wool_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_589, result: Stack { item: 292, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:purple_wool_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_590, result: Stack { item: 276, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:purpur_block", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_591, result: Stack { item: 399, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:purpur_pillar", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_592, result: Stack { item: 400, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:purpur_pillar_from_purpur_block_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_569, result: Stack { item: 400, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:purpur_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_593, result: Stack { item: 368, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:purpur_slab_from_purpur_block_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_569, result: Stack { item: 368, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:purpur_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_594, result: Stack { item: 401, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:purpur_stairs_from_purpur_block_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_569, result: Stack { item: 401, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:quartz", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_571, result: Stack { item: 1015, count: 1, components: &[0, 0] }, experience: 0.2, time: 200 } },
    Recipe { id: "minecraft:quartz_block", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_595, result: Stack { item: 557, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:quartz_bricks", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_596, result: Stack { item: 558, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:quartz_bricks_from_quartz_block_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_149, result: Stack { item: 558, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:quartz_from_blasting", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_571, result: Stack { item: 1015, count: 1, components: &[0, 0] }, experience: 0.2, time: 200 } },
    Recipe { id: "minecraft:quartz_pillar", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_597, result: Stack { item: 559, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:quartz_pillar_from_quartz_block_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_149, result: Stack { item: 559, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:quartz_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_598, result: Stack { item: 365, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:quartz_slab_from_quartz_block_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_149, result: Stack { item: 365, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:quartz_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_599, result: Stack { item: 560, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:quartz_stairs_from_quartz_block_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_149, result: Stack { item: 560, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:rabbit_stew_from_brown_mushroom", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_259, result: Stack { item: 1402, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:rabbit_stew_from_red_mushroom", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_260, result: Stack { item: 1402, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:rabbit_stew_from_shelf_mushroom", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_261, result: Stack { item: 1402, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:rail", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_600, result: Stack { item: 947, count: 16, components: &[0, 0] } } },
    Recipe { id: "minecraft:raiser_armor_trim_smithing_template", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_601, result: Stack { item: 1594, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:raiser_armor_trim_smithing_template_smithing_trim", kind: RecipeKind::SmithingTrim { template: INGREDIENT_577, base: INGREDIENT_73, addition: INGREDIENT_74, pattern: "minecraft:raiser" } },
    Recipe { id: "minecraft:raw_copper", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_262, result: Stack { item: 1019, count: 9, components: &[0, 0] } } },
    Recipe { id: "minecraft:raw_copper_block", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_602, result: Stack { item: 114, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:raw_gold", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_263, result: Stack { item: 1021, count: 9, components: &[0, 0] } } },
    Recipe { id: "minecraft:raw_gold_block", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_603, result: Stack { item: 115, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:raw_iron", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_264, result: Stack { item: 1017, count: 9, components: &[0, 0] } } },
    Recipe { id: "minecraft:raw_iron_block", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_604, result: Stack { item: 113, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:recovery_compass", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_605, result: Stack { item: 1152, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_banner", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_606, result: Stack { item: 1431, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_banner_duplicate", kind: RecipeKind::BannerDuplicate { banner: INGREDIENT_583, result: Stack { item: 1431, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_bed", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_607, result: Stack { item: 1233, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_bundle", kind: RecipeKind::Transmute { input: INGREDIENT_41, material: INGREDIENT_337, min_material: 1, max_material: 1, result_item: Some(1168), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:red_candle", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_265, result: Stack { item: 1565, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_carpet", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_608, result: Stack { item: 594, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_concrete_powder", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_266, result: Stack { item: 751, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_concrete_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_609, result: Stack { item: 735, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_concrete_slab_from_red_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_584, result: Stack { item: 735, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_concrete_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_610, result: Stack { item: 719, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_concrete_stairs_from_red_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_584, result: Stack { item: 719, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_cushion", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_611, result: Stack { item: 1184, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_dye_from_beetroot", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_267, result: Stack { item: 1213, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_dye_from_poppy", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_268, result: Stack { item: 1213, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_dye_from_rose_bush", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_269, result: Stack { item: 1213, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_dye_from_tulip", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_270, result: Stack { item: 1213, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_glazed_terracotta", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_589, result: Stack { item: 687, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:red_harness", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_612, result: Stack { item: 964, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_nether_brick_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_613, result: Stack { item: 816, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_nether_brick_slab_from_red_nether_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_590, result: Stack { item: 816, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_nether_brick_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_614, result: Stack { item: 799, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_nether_brick_stairs_from_red_nether_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_590, result: Stack { item: 799, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_nether_brick_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_615, result: Stack { item: 542, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_nether_brick_wall_from_red_nether_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_590, result: Stack { item: 542, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_nether_bricks", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_616, result: Stack { item: 653, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_sandstone", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_617, result: Stack { item: 644, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_sandstone_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_618, result: Stack { item: 366, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_sandstone_slab_from_red_sandstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_151, result: Stack { item: 366, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_sandstone_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_619, result: Stack { item: 647, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_sandstone_stairs_from_red_sandstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_151, result: Stack { item: 647, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_sandstone_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_620, result: Stack { item: 535, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_sandstone_wall_from_red_sandstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_151, result: Stack { item: 535, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_shulker_box", kind: RecipeKind::Transmute { input: INGREDIENT_52, material: INGREDIENT_337, min_material: 1, max_material: 1, result_item: Some(671), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:red_stained_glass", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_621, result: Stack { item: 619, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_stained_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_622, result: Stack { item: 635, count: 16, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_stained_glass_pane_from_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_623, result: Stack { item: 635, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_terracotta", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_624, result: Stack { item: 575, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_wool_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_625, result: Stack { item: 296, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:red_wool_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_626, result: Stack { item: 280, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:redstone", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_271, result: Stack { item: 824, count: 9, components: &[0, 0] } } },
    Recipe { id: "minecraft:redstone_block", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_627, result: Stack { item: 826, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:redstone_from_blasting_deepslate_redstone_ore", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_596, result: Stack { item: 824, count: 1, components: &[0, 0] }, experience: 0.7, time: 200 } },
    Recipe { id: "minecraft:redstone_from_blasting_redstone_ore", kind: RecipeKind::Cooking { kind: CookingKind::Blasting, ingredient: INGREDIENT_597, result: Stack { item: 824, count: 1, components: &[0, 0] }, experience: 0.7, time: 200 } },
    Recipe { id: "minecraft:redstone_from_smelting_deepslate_redstone_ore", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_596, result: Stack { item: 824, count: 1, components: &[0, 0] }, experience: 0.7, time: 200 } },
    Recipe { id: "minecraft:redstone_from_smelting_redstone_ore", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_597, result: Stack { item: 824, count: 1, components: &[0, 0] }, experience: 0.7, time: 200 } },
    Recipe { id: "minecraft:redstone_lamp", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_628, result: Stack { item: 854, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:redstone_torch", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_629, result: Stack { item: 825, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:repair_item", kind: RecipeKind::RepairItem },
    Recipe { id: "minecraft:repeater", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_630, result: Stack { item: 827, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:resin_block", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_631, result: Stack { item: 487, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:resin_brick", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_599, result: Stack { item: 1397, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:resin_brick_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_632, result: Stack { item: 490, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:resin_brick_slab_from_resin_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_153, result: Stack { item: 490, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:resin_brick_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_633, result: Stack { item: 489, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:resin_brick_stairs_from_resin_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_153, result: Stack { item: 489, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:resin_brick_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_634, result: Stack { item: 491, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:resin_brick_wall_from_resin_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_153, result: Stack { item: 491, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:resin_bricks", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_635, result: Stack { item: 488, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:resin_clump", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_272, result: Stack { item: 486, count: 9, components: &[0, 0] } } },
    Recipe { id: "minecraft:respawn_anchor", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_636, result: Stack { item: 1549, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:rib_armor_trim_smithing_template", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_637, result: Stack { item: 1589, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:rib_armor_trim_smithing_template_smithing_trim", kind: RecipeKind::SmithingTrim { template: INGREDIENT_602, base: INGREDIENT_73, addition: INGREDIENT_74, pattern: "minecraft:rib" } },
    Recipe { id: "minecraft:saddle", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_638, result: Stack { item: 949, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:sandstone", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_639, result: Stack { item: 234, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:sandstone_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_640, result: Stack { item: 357, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:sandstone_slab_from_sandstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_155, result: Stack { item: 357, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:sandstone_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_641, result: Stack { item: 512, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:sandstone_stairs_from_sandstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_155, result: Stack { item: 512, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:sandstone_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_642, result: Stack { item: 543, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:sandstone_wall_from_sandstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_155, result: Stack { item: 543, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:scaffolding", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_643, result: Stack { item: 823, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:sea_lantern", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_644, result: Stack { item: 643, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:sentry_armor_trim_smithing_template", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_645, result: Stack { item: 1580, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:sentry_armor_trim_smithing_template_smithing_trim", kind: RecipeKind::SmithingTrim { template: INGREDIENT_606, base: INGREDIENT_73, addition: INGREDIENT_74, pattern: "minecraft:sentry" } },
    Recipe { id: "minecraft:shaper_armor_trim_smithing_template", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_646, result: Stack { item: 1592, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:shaper_armor_trim_smithing_template_smithing_trim", kind: RecipeKind::SmithingTrim { template: INGREDIENT_607, base: INGREDIENT_73, addition: INGREDIENT_74, pattern: "minecraft:shaper" } },
    Recipe { id: "minecraft:shears", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_647, result: Stack { item: 1255, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:shield", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_648, result: Stack { item: 1446, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:shield_decoration", kind: RecipeKind::ShieldDecoration { banner: INGREDIENT_609, target: INGREDIENT_610, result: Stack { item: 1446, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:shulker_box", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_649, result: Stack { item: 656, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:silence_armor_trim_smithing_template", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_650, result: Stack { item: 1593, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:silence_armor_trim_smithing_template_smithing_trim", kind: RecipeKind::SmithingTrim { template: INGREDIENT_612, base: INGREDIENT_73, addition: INGREDIENT_74, pattern: "minecraft:silence" } },
    Recipe { id: "minecraft:skull_banner_pattern", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_273, result: Stack { item: 1496, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:slime_ball", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_274, result: Stack { item: 1147, count: 9, components: &[0, 0] } } },
    Recipe { id: "minecraft:slime_block", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_651, result: Stack { item: 831, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:smithing_table", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_652, result: Stack { item: 1512, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:smoker", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_653, result: Stack { item: 1507, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:smooth_basalt", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_549, result: Stack { item: 438, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:smooth_quartz", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_149, result: Stack { item: 372, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:smooth_quartz_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_654, result: Stack { item: 813, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:smooth_quartz_slab_from_smooth_quartz_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_614, result: Stack { item: 813, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:smooth_quartz_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_655, result: Stack { item: 796, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:smooth_quartz_stairs_from_smooth_quartz_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_614, result: Stack { item: 796, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:smooth_red_sandstone", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_151, result: Stack { item: 373, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:smooth_red_sandstone_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_656, result: Stack { item: 807, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:smooth_red_sandstone_slab_from_smooth_red_sandstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_615, result: Stack { item: 807, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:smooth_red_sandstone_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_657, result: Stack { item: 789, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:smooth_red_sandstone_stairs_from_smooth_red_sandstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_615, result: Stack { item: 789, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:smooth_sandstone", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_155, result: Stack { item: 374, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:smooth_sandstone_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_658, result: Stack { item: 812, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:smooth_sandstone_slab_from_smooth_sandstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_616, result: Stack { item: 812, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:smooth_sandstone_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_659, result: Stack { item: 795, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:smooth_sandstone_stairs_from_smooth_sandstone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_616, result: Stack { item: 795, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:smooth_stone", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_98, result: Stack { item: 375, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:smooth_stone_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_660, result: Stack { item: 356, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:smooth_stone_slab_from_smooth_stone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_58, result: Stack { item: 356, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:snout_armor_trim_smithing_template", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_661, result: Stack { item: 1588, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:snout_armor_trim_smithing_template_smithing_trim", kind: RecipeKind::SmithingTrim { template: INGREDIENT_617, base: INGREDIENT_73, addition: INGREDIENT_74, pattern: "minecraft:snout" } },
    Recipe { id: "minecraft:snow", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_662, result: Stack { item: 410, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:snow_block", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_663, result: Stack { item: 412, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:soul_campfire", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_664, result: Stack { item: 1528, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:soul_lantern", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_665, result: Stack { item: 1516, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:soul_torch", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_666, result: Stack { item: 439, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:spectral_arrow", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_667, result: Stack { item: 1443, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:spire_armor_trim_smithing_template", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_668, result: Stack { item: 1590, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:spire_armor_trim_smithing_template_smithing_trim", kind: RecipeKind::SmithingTrim { template: INGREDIENT_623, base: INGREDIENT_73, addition: INGREDIENT_74, pattern: "minecraft:spire" } },
    Recipe { id: "minecraft:sponge", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_624, result: Stack { item: 229, count: 1, components: &[0, 0] }, experience: 0.15, time: 200 } },
    Recipe { id: "minecraft:spruce_boat", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_669, result: Stack { item: 977, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:spruce_button", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_275, result: Stack { item: 859, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:spruce_chest_boat", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_276, result: Stack { item: 978, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:spruce_door", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_670, result: Stack { item: 890, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:spruce_fence", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_671, result: Stack { item: 418, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:spruce_fence_gate", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_672, result: Stack { item: 933, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:spruce_hanging_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_673, result: Stack { item: 1116, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:spruce_planks", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_277, result: Stack { item: 64, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:spruce_pressure_plate", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_674, result: Stack { item: 876, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:spruce_shelf", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_675, result: Stack { item: 388, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:spruce_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_676, result: Stack { item: 1103, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:spruce_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_677, result: Stack { item: 342, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:spruce_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_678, result: Stack { item: 516, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:spruce_trapdoor", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_670, result: Stack { item: 912, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:spruce_wood", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_679, result: Stack { item: 204, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:spyglass", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_680, result: Stack { item: 1188, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:stick", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_681, result: Stack { item: 1060, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:stick_from_bamboo_item", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_682, result: Stack { item: 1060, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:sticky_piston", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_683, result: Stack { item: 830, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_12, result: Stack { item: 1, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:stone_axe", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_684, result: Stack { item: 1038, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone_brick_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_685, result: Stack { item: 362, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone_brick_slab_from_stone_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_157, result: Stack { item: 362, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone_brick_slab_from_stone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_98, result: Stack { item: 362, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone_brick_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_686, result: Stack { item: 494, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone_brick_stairs_from_stone_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_157, result: Stack { item: 494, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone_brick_stairs_from_stone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_98, result: Stack { item: 494, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone_brick_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_687, result: Stack { item: 538, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone_brick_wall_from_stone_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_157, result: Stack { item: 538, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone_brick_wall_from_stone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_98, result: Stack { item: 538, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone_bricks", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_688, result: Stack { item: 449, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone_bricks_from_stone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_98, result: Stack { item: 449, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone_button", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_278, result: Stack { item: 856, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone_hoe", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_689, result: Stack { item: 1039, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone_pickaxe", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_690, result: Stack { item: 1037, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone_pressure_plate", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_691, result: Stack { item: 871, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone_shovel", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_692, result: Stack { item: 1036, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_693, result: Stack { item: 355, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone_slab_from_stone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_98, result: Stack { item: 355, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone_spear", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_694, result: Stack { item: 1448, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_695, result: Stack { item: 794, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone_stairs_from_stone_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_98, result: Stack { item: 794, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:stone_sword", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_696, result: Stack { item: 1035, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:stonecutter", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_697, result: Stack { item: 1513, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:straw_bed", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_698, result: Stack { item: 1235, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:stripped_acacia_wood", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_699, result: Stack { item: 194, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:stripped_birch_wood", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_700, result: Stack { item: 192, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:stripped_cherry_wood", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_701, result: Stack { item: 195, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:stripped_crimson_hyphae", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_702, result: Stack { item: 200, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:stripped_dark_oak_wood", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_703, result: Stack { item: 196, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:stripped_jungle_wood", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_704, result: Stack { item: 193, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:stripped_mangrove_wood", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_705, result: Stack { item: 198, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:stripped_oak_wood", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_706, result: Stack { item: 190, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:stripped_pale_oak_wood", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_707, result: Stack { item: 197, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:stripped_poplar_wood", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_708, result: Stack { item: 199, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:stripped_spruce_wood", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_709, result: Stack { item: 191, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:stripped_warped_hyphae", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_710, result: Stack { item: 201, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:sugar_from_honey_bottle", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_279, result: Stack { item: 1217, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:sugar_from_sugar_cane", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_280, result: Stack { item: 1217, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:sulfur_brick_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_711, result: Stack { item: 36, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:sulfur_brick_slab_from_polished_sulfur_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_552, result: Stack { item: 36, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:sulfur_brick_slab_from_sulfur_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_634, result: Stack { item: 36, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:sulfur_brick_slab_from_sulfur_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_159, result: Stack { item: 36, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:sulfur_brick_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_712, result: Stack { item: 37, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:sulfur_brick_stairs_from_polished_sulfur_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_552, result: Stack { item: 37, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:sulfur_brick_stairs_from_sulfur_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_634, result: Stack { item: 37, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:sulfur_brick_stairs_from_sulfur_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_159, result: Stack { item: 37, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:sulfur_brick_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_713, result: Stack { item: 38, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:sulfur_brick_wall_from_polished_sulfur_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_552, result: Stack { item: 38, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:sulfur_brick_wall_from_sulfur_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_634, result: Stack { item: 38, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:sulfur_brick_wall_from_sulfur_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_159, result: Stack { item: 38, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:sulfur_bricks", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_714, result: Stack { item: 35, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:sulfur_bricks_from_polished_sulfur_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_552, result: Stack { item: 35, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:sulfur_bricks_from_sulfur_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_159, result: Stack { item: 35, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:sulfur_from_sulfur_spikes", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_715, result: Stack { item: 26, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:sulfur_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_716, result: Stack { item: 28, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:sulfur_slab_from_sulfur_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_159, result: Stack { item: 28, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:sulfur_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_717, result: Stack { item: 29, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:sulfur_stairs_from_sulfur_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_159, result: Stack { item: 29, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:sulfur_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_718, result: Stack { item: 30, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:sulfur_wall_from_sulfur_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_159, result: Stack { item: 30, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:suspicious_stew_from_allium", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_281, result: Stack { item: 1492, count: 1, components: &[1, 0, 55, 1, 11, 60] } } },
    Recipe { id: "minecraft:suspicious_stew_from_azure_bluet", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_282, result: Stack { item: 1492, count: 1, components: &[1, 0, 55, 1, 14, 220, 1] } } },
    Recipe { id: "minecraft:suspicious_stew_from_blue_orchid", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_283, result: Stack { item: 1492, count: 1, components: &[1, 0, 55, 1, 22, 7] } } },
    Recipe { id: "minecraft:suspicious_stew_from_closed_eyeblossom", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_284, result: Stack { item: 1492, count: 1, components: &[1, 0, 55, 1, 8, 140, 1] } } },
    Recipe { id: "minecraft:suspicious_stew_from_cornflower", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_285, result: Stack { item: 1492, count: 1, components: &[1, 0, 55, 1, 7, 100] } } },
    Recipe { id: "minecraft:suspicious_stew_from_dandelion", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_286, result: Stack { item: 1492, count: 1, components: &[1, 0, 55, 1, 22, 7] } } },
    Recipe { id: "minecraft:suspicious_stew_from_golden_dandelion", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_287, result: Stack { item: 1492, count: 1, components: &[1, 0, 55, 1, 22, 7] } } },
    Recipe { id: "minecraft:suspicious_stew_from_lily_of_the_valley", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_288, result: Stack { item: 1492, count: 1, components: &[1, 0, 55, 1, 18, 220, 1] } } },
    Recipe { id: "minecraft:suspicious_stew_from_open_eyeblossom", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_289, result: Stack { item: 1492, count: 1, components: &[1, 0, 55, 1, 14, 220, 1] } } },
    Recipe { id: "minecraft:suspicious_stew_from_orange_tulip", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_290, result: Stack { item: 1492, count: 1, components: &[1, 0, 55, 1, 17, 140, 1] } } },
    Recipe { id: "minecraft:suspicious_stew_from_oxeye_daisy", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_291, result: Stack { item: 1492, count: 1, components: &[1, 0, 55, 1, 9, 140, 1] } } },
    Recipe { id: "minecraft:suspicious_stew_from_pink_tulip", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_292, result: Stack { item: 1492, count: 1, components: &[1, 0, 55, 1, 17, 140, 1] } } },
    Recipe { id: "minecraft:suspicious_stew_from_poppy", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_293, result: Stack { item: 1492, count: 1, components: &[1, 0, 55, 1, 15, 100] } } },
    Recipe { id: "minecraft:suspicious_stew_from_red_tulip", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_294, result: Stack { item: 1492, count: 1, components: &[1, 0, 55, 1, 17, 140, 1] } } },
    Recipe { id: "minecraft:suspicious_stew_from_torchflower", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_295, result: Stack { item: 1492, count: 1, components: &[1, 0, 55, 1, 15, 100] } } },
    Recipe { id: "minecraft:suspicious_stew_from_white_tulip", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_296, result: Stack { item: 1492, count: 1, components: &[1, 0, 55, 1, 17, 140, 1] } } },
    Recipe { id: "minecraft:suspicious_stew_from_wither_rose", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_297, result: Stack { item: 1492, count: 1, components: &[1, 0, 55, 1, 19, 140, 1] } } },
    Recipe { id: "minecraft:target", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_719, result: Stack { item: 838, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:terracotta", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_638, result: Stack { item: 596, count: 1, components: &[0, 0] }, experience: 0.35, time: 200 } },
    Recipe { id: "minecraft:tide_armor_trim_smithing_template", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_720, result: Stack { item: 1587, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:tide_armor_trim_smithing_template_smithing_trim", kind: RecipeKind::SmithingTrim { template: INGREDIENT_639, base: INGREDIENT_73, addition: INGREDIENT_74, pattern: "minecraft:tide" } },
    Recipe { id: "minecraft:tinted_glass", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_721, result: Stack { item: 232, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:tipped_arrow", kind: RecipeKind::Imbue { source: INGREDIENT_85, material: INGREDIENT_622, result: Stack { item: 1444, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:tnt", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_722, result: Stack { item: 853, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:tnt_minecart", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_298, result: Stack { item: 969, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:torch", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_723, result: Stack { item: 395, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:trapped_chest", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_299, result: Stack { item: 852, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:tripwire_hook", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_724, result: Stack { item: 851, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:tuff_brick_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_725, result: Stack { item: 22, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:tuff_brick_slab_from_polished_tuff_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_162, result: Stack { item: 22, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:tuff_brick_slab_from_tuff_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_163, result: Stack { item: 22, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:tuff_brick_slab_from_tuff_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_164, result: Stack { item: 22, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:tuff_brick_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_726, result: Stack { item: 23, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:tuff_brick_stairs_from_polished_tuff_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_162, result: Stack { item: 23, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:tuff_brick_stairs_from_tuff_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_163, result: Stack { item: 23, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:tuff_brick_stairs_from_tuff_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_164, result: Stack { item: 23, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:tuff_brick_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_727, result: Stack { item: 24, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:tuff_brick_wall_from_polished_tuff_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_162, result: Stack { item: 24, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:tuff_brick_wall_from_tuff_bricks_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_163, result: Stack { item: 24, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:tuff_brick_wall_from_tuff_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_164, result: Stack { item: 24, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:tuff_bricks", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_728, result: Stack { item: 21, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:tuff_bricks_from_polished_tuff_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_162, result: Stack { item: 21, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:tuff_bricks_from_tuff_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_164, result: Stack { item: 21, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:tuff_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_729, result: Stack { item: 13, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:tuff_slab_from_tuff_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_164, result: Stack { item: 13, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:tuff_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_730, result: Stack { item: 14, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:tuff_stairs_from_tuff_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_164, result: Stack { item: 14, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:tuff_wall", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_731, result: Stack { item: 15, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:tuff_wall_from_tuff_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_164, result: Stack { item: 15, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:turtle_helmet", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_732, result: Stack { item: 1001, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:vex_armor_trim_smithing_template", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_733, result: Stack { item: 1586, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:vex_armor_trim_smithing_template_smithing_trim", kind: RecipeKind::SmithingTrim { template: INGREDIENT_643, base: INGREDIENT_73, addition: INGREDIENT_74, pattern: "minecraft:vex" } },
    Recipe { id: "minecraft:ward_armor_trim_smithing_template", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_734, result: Stack { item: 1584, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:ward_armor_trim_smithing_template_smithing_trim", kind: RecipeKind::SmithingTrim { template: INGREDIENT_644, base: INGREDIENT_73, addition: INGREDIENT_74, pattern: "minecraft:ward" } },
    Recipe { id: "minecraft:warped_button", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_300, result: Stack { item: 870, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:warped_door", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_735, result: Stack { item: 901, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:warped_fence", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_736, result: Stack { item: 429, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:warped_fence_gate", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_737, result: Stack { item: 944, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:warped_fungus_on_a_stick", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_738, result: Stack { item: 972, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:warped_hanging_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_739, result: Stack { item: 1127, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:warped_hyphae", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_740, result: Stack { item: 214, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:warped_planks", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_301, result: Stack { item: 75, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:warped_pressure_plate", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_741, result: Stack { item: 887, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:warped_shelf", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_742, result: Stack { item: 389, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:warped_sign", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_743, result: Stack { item: 1114, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:warped_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_744, result: Stack { item: 354, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:warped_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_745, result: Stack { item: 528, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:warped_trapdoor", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_735, result: Stack { item: 923, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_chiseled_copper", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_746, result: Stack { item: 135, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_chiseled_copper_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_302, result: Stack { item: 135, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_chiseled_copper_from_waxed_copper_block_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_189, result: Stack { item: 135, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_chiseled_copper_from_waxed_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_651, result: Stack { item: 135, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_copper_bars_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_303, result: Stack { item: 469, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_copper_block_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_304, result: Stack { item: 124, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_copper_bulb", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_747, result: Stack { item: 1633, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_copper_bulb_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_305, result: Stack { item: 1633, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_copper_chain_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_306, result: Stack { item: 478, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_copper_chest_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_307, result: Stack { item: 1641, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_copper_door_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_308, result: Stack { item: 906, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_copper_golem_statue_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_309, result: Stack { item: 1649, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_copper_grate", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_748, result: Stack { item: 1625, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_copper_grate_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_310, result: Stack { item: 1625, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_copper_grate_from_waxed_copper_block_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_189, result: Stack { item: 1625, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_copper_lantern_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_311, result: Stack { item: 1521, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_copper_trapdoor_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_312, result: Stack { item: 928, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_cut_copper", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_749, result: Stack { item: 143, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_cut_copper_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_313, result: Stack { item: 143, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_cut_copper_from_waxed_copper_block_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_189, result: Stack { item: 143, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_cut_copper_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_750, result: Stack { item: 159, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_cut_copper_slab_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_314, result: Stack { item: 159, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_cut_copper_slab_from_waxed_copper_block_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_189, result: Stack { item: 159, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_cut_copper_slab_from_waxed_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_651, result: Stack { item: 159, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_cut_copper_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_751, result: Stack { item: 151, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_cut_copper_stairs_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_315, result: Stack { item: 151, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_cut_copper_stairs_from_waxed_copper_block_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_189, result: Stack { item: 151, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_cut_copper_stairs_from_waxed_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_651, result: Stack { item: 151, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_chiseled_copper", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_752, result: Stack { item: 136, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_chiseled_copper_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_316, result: Stack { item: 136, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_chiseled_copper_from_waxed_exposed_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_664, result: Stack { item: 136, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_chiseled_copper_from_waxed_exposed_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_665, result: Stack { item: 136, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_copper_bars_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_317, result: Stack { item: 470, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_copper_bulb", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_753, result: Stack { item: 1634, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_copper_bulb_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_318, result: Stack { item: 1634, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_copper_chain_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_319, result: Stack { item: 479, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_copper_chest_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_320, result: Stack { item: 1642, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_copper_door_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_321, result: Stack { item: 907, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_copper_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_322, result: Stack { item: 125, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_copper_golem_statue_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_323, result: Stack { item: 1650, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_copper_grate", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_754, result: Stack { item: 1626, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_copper_grate_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_324, result: Stack { item: 1626, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_copper_grate_from_waxed_exposed_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_664, result: Stack { item: 1626, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_copper_lantern_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_325, result: Stack { item: 1522, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_copper_trapdoor_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_326, result: Stack { item: 929, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_cut_copper", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_755, result: Stack { item: 144, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_cut_copper_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_327, result: Stack { item: 144, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_cut_copper_from_waxed_exposed_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_664, result: Stack { item: 144, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_cut_copper_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_756, result: Stack { item: 160, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_cut_copper_slab_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_328, result: Stack { item: 160, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_cut_copper_slab_from_waxed_exposed_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_664, result: Stack { item: 160, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_cut_copper_slab_from_waxed_exposed_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_665, result: Stack { item: 160, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_cut_copper_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_757, result: Stack { item: 152, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_cut_copper_stairs_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_329, result: Stack { item: 152, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_cut_copper_stairs_from_waxed_exposed_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_664, result: Stack { item: 152, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_cut_copper_stairs_from_waxed_exposed_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_665, result: Stack { item: 152, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_exposed_lightning_rod_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_330, result: Stack { item: 845, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_lightning_rod_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_331, result: Stack { item: 844, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_chiseled_copper", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_758, result: Stack { item: 138, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_chiseled_copper_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_332, result: Stack { item: 138, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_chiseled_copper_from_waxed_oxidized_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_680, result: Stack { item: 138, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_chiseled_copper_from_waxed_oxidized_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_681, result: Stack { item: 138, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_copper_bars_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_333, result: Stack { item: 472, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_copper_bulb", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_759, result: Stack { item: 1636, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_copper_bulb_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_334, result: Stack { item: 1636, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_copper_chain_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_335, result: Stack { item: 481, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_copper_chest_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_336, result: Stack { item: 1644, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_copper_door_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_337, result: Stack { item: 909, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_copper_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_338, result: Stack { item: 127, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_copper_golem_statue_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_339, result: Stack { item: 1652, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_copper_grate", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_760, result: Stack { item: 1628, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_copper_grate_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_340, result: Stack { item: 1628, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_copper_grate_from_waxed_oxidized_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_680, result: Stack { item: 1628, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_copper_lantern_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_341, result: Stack { item: 1524, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_copper_trapdoor_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_342, result: Stack { item: 931, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_cut_copper", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_761, result: Stack { item: 146, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_cut_copper_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_343, result: Stack { item: 146, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_cut_copper_from_waxed_oxidized_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_680, result: Stack { item: 146, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_cut_copper_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_762, result: Stack { item: 162, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_cut_copper_slab_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_344, result: Stack { item: 162, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_cut_copper_slab_from_waxed_oxidized_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_680, result: Stack { item: 162, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_cut_copper_slab_from_waxed_oxidized_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_681, result: Stack { item: 162, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_cut_copper_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_763, result: Stack { item: 154, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_cut_copper_stairs_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_345, result: Stack { item: 154, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_cut_copper_stairs_from_waxed_oxidized_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_680, result: Stack { item: 154, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_cut_copper_stairs_from_waxed_oxidized_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_681, result: Stack { item: 154, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_oxidized_lightning_rod_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_346, result: Stack { item: 847, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_chiseled_copper", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_764, result: Stack { item: 137, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_chiseled_copper_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_347, result: Stack { item: 137, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_chiseled_copper_from_waxed_weathered_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_695, result: Stack { item: 137, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_chiseled_copper_from_waxed_weathered_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_696, result: Stack { item: 137, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_copper_bars_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_348, result: Stack { item: 471, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_copper_bulb", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_765, result: Stack { item: 1635, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_copper_bulb_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_349, result: Stack { item: 1635, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_copper_chain_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_350, result: Stack { item: 480, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_copper_chest_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_351, result: Stack { item: 1643, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_copper_door_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_352, result: Stack { item: 908, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_copper_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_353, result: Stack { item: 126, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_copper_golem_statue_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_354, result: Stack { item: 1651, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_copper_grate", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_766, result: Stack { item: 1627, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_copper_grate_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_355, result: Stack { item: 1627, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_copper_grate_from_waxed_weathered_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_695, result: Stack { item: 1627, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_copper_lantern_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_356, result: Stack { item: 1523, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_copper_trapdoor_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_357, result: Stack { item: 930, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_cut_copper", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_767, result: Stack { item: 145, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_cut_copper_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_358, result: Stack { item: 145, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_cut_copper_from_waxed_weathered_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_695, result: Stack { item: 145, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_cut_copper_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_768, result: Stack { item: 161, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_cut_copper_slab_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_359, result: Stack { item: 161, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_cut_copper_slab_from_waxed_weathered_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_695, result: Stack { item: 161, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_cut_copper_slab_from_waxed_weathered_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_696, result: Stack { item: 161, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_cut_copper_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_769, result: Stack { item: 153, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_cut_copper_stairs_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_360, result: Stack { item: 153, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_cut_copper_stairs_from_waxed_weathered_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_695, result: Stack { item: 153, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_cut_copper_stairs_from_waxed_weathered_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_696, result: Stack { item: 153, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:waxed_weathered_lightning_rod_from_honeycomb", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_361, result: Stack { item: 846, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:wayfinder_armor_trim_smithing_template", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_770, result: Stack { item: 1591, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:wayfinder_armor_trim_smithing_template_smithing_trim", kind: RecipeKind::SmithingTrim { template: INGREDIENT_711, base: INGREDIENT_73, addition: INGREDIENT_74, pattern: "minecraft:wayfinder" } },
    Recipe { id: "minecraft:weathered_chiseled_copper", kind: RecipeKind::Shaped { width: 1, height: 2, pattern: PATTERN_771, result: Stack { item: 133, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:weathered_chiseled_copper_from_weathered_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_702, result: Stack { item: 133, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:weathered_chiseled_copper_from_weathered_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_707, result: Stack { item: 133, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:weathered_copper_bulb", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_772, result: Stack { item: 1631, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:weathered_copper_grate", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_773, result: Stack { item: 1623, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:weathered_copper_grate_from_weathered_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_702, result: Stack { item: 1623, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:weathered_cut_copper", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_774, result: Stack { item: 141, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:weathered_cut_copper_from_weathered_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_702, result: Stack { item: 141, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:weathered_cut_copper_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_775, result: Stack { item: 157, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:weathered_cut_copper_slab_from_weathered_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_702, result: Stack { item: 157, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:weathered_cut_copper_slab_from_weathered_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_707, result: Stack { item: 157, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:weathered_cut_copper_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_776, result: Stack { item: 149, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:weathered_cut_copper_stairs_from_weathered_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_702, result: Stack { item: 149, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:weathered_cut_copper_stairs_from_weathered_cut_copper_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_707, result: Stack { item: 149, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:wheat", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_362, result: Stack { item: 1066, count: 9, components: &[0, 0] } } },
    Recipe { id: "minecraft:white_banner", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_777, result: Stack { item: 1417, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:white_banner_duplicate", kind: RecipeKind::BannerDuplicate { banner: INGREDIENT_713, result: Stack { item: 1417, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:white_bed", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_778, result: Stack { item: 1219, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:white_bundle", kind: RecipeKind::Transmute { input: INGREDIENT_41, material: INGREDIENT_345, min_material: 1, max_material: 1, result_item: Some(1154), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:white_candle", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_363, result: Stack { item: 1551, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:white_carpet", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_779, result: Stack { item: 580, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:white_concrete_powder", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_364, result: Stack { item: 737, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:white_concrete_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_780, result: Stack { item: 721, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:white_concrete_slab_from_white_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_714, result: Stack { item: 721, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:white_concrete_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_781, result: Stack { item: 705, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:white_concrete_stairs_from_white_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_714, result: Stack { item: 705, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:white_cushion", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_782, result: Stack { item: 1170, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:white_dye", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_365, result: Stack { item: 1199, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:white_dye_from_lily_of_the_valley", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_366, result: Stack { item: 1199, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:white_glazed_terracotta", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_716, result: Stack { item: 673, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:white_harness", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_783, result: Stack { item: 950, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:white_shulker_box", kind: RecipeKind::Transmute { input: INGREDIENT_52, material: INGREDIENT_345, min_material: 1, max_material: 1, result_item: Some(657), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:white_stained_glass", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_784, result: Stack { item: 605, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:white_stained_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_785, result: Stack { item: 621, count: 16, components: &[0, 0] } } },
    Recipe { id: "minecraft:white_stained_glass_pane_from_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_786, result: Stack { item: 621, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:white_terracotta", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_787, result: Stack { item: 561, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:white_wool_from_string", kind: RecipeKind::Shaped { width: 2, height: 2, pattern: PATTERN_788, result: Stack { item: 250, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:white_wool_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_789, result: Stack { item: 282, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:white_wool_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_790, result: Stack { item: 266, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:wild_armor_trim_smithing_template", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_791, result: Stack { item: 1583, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:wild_armor_trim_smithing_template_smithing_trim", kind: RecipeKind::SmithingTrim { template: INGREDIENT_718, base: INGREDIENT_73, addition: INGREDIENT_74, pattern: "minecraft:wild" } },
    Recipe { id: "minecraft:wind_charge", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_367, result: Stack { item: 1370, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:wolf_armor", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_792, result: Stack { item: 1004, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:wolf_armor_dyed", kind: RecipeKind::Dye { target: INGREDIENT_720, dye: INGREDIENT_379, result: Stack { item: 1004, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:wooden_axe", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_793, result: Stack { item: 1028, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:wooden_hoe", kind: RecipeKind::Shaped { width: 2, height: 3, pattern: PATTERN_794, result: Stack { item: 1029, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:wooden_pickaxe", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_795, result: Stack { item: 1027, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:wooden_shovel", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_796, result: Stack { item: 1026, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:wooden_spear", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_797, result: Stack { item: 1447, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:wooden_sword", kind: RecipeKind::Shaped { width: 1, height: 3, pattern: PATTERN_798, result: Stack { item: 1025, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:writable_book", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_368, result: Stack { item: 1371, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:yellow_banner", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_799, result: Stack { item: 1421, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:yellow_banner_duplicate", kind: RecipeKind::BannerDuplicate { banner: INGREDIENT_722, result: Stack { item: 1421, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:yellow_bed", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_800, result: Stack { item: 1223, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:yellow_bundle", kind: RecipeKind::Transmute { input: INGREDIENT_41, material: INGREDIENT_353, min_material: 1, max_material: 1, result_item: Some(1158), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:yellow_candle", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_369, result: Stack { item: 1555, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:yellow_carpet", kind: RecipeKind::Shaped { width: 2, height: 1, pattern: PATTERN_801, result: Stack { item: 584, count: 3, components: &[0, 0] } } },
    Recipe { id: "minecraft:yellow_concrete_powder", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_370, result: Stack { item: 741, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:yellow_concrete_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_802, result: Stack { item: 725, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:yellow_concrete_slab_from_yellow_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_723, result: Stack { item: 725, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:yellow_concrete_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_803, result: Stack { item: 709, count: 4, components: &[0, 0] } } },
    Recipe { id: "minecraft:yellow_concrete_stairs_from_yellow_concrete_stonecutting", kind: RecipeKind::Stonecutting { ingredient: INGREDIENT_723, result: Stack { item: 709, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:yellow_cushion", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_804, result: Stack { item: 1174, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:yellow_dye_from_dandelion", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_371, result: Stack { item: 1203, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:yellow_dye_from_golden_dandelion", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_372, result: Stack { item: 1203, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:yellow_dye_from_sunflower", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_373, result: Stack { item: 1203, count: 2, components: &[0, 0] } } },
    Recipe { id: "minecraft:yellow_dye_from_wildflowers", kind: RecipeKind::Shapeless { ingredients: INGREDIENTS_374, result: Stack { item: 1203, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:yellow_glazed_terracotta", kind: RecipeKind::Cooking { kind: CookingKind::Smelting, ingredient: INGREDIENT_727, result: Stack { item: 677, count: 1, components: &[0, 0] }, experience: 0.1, time: 200 } },
    Recipe { id: "minecraft:yellow_harness", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_805, result: Stack { item: 954, count: 1, components: &[0, 0] } } },
    Recipe { id: "minecraft:yellow_shulker_box", kind: RecipeKind::Transmute { input: INGREDIENT_52, material: INGREDIENT_353, min_material: 1, max_material: 1, result_item: Some(661), result_count: 1, result_components: &[0, 0], add_material_count: false } },
    Recipe { id: "minecraft:yellow_stained_glass", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_806, result: Stack { item: 609, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:yellow_stained_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 2, pattern: PATTERN_807, result: Stack { item: 625, count: 16, components: &[0, 0] } } },
    Recipe { id: "minecraft:yellow_stained_glass_pane_from_glass_pane", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_808, result: Stack { item: 625, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:yellow_terracotta", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_809, result: Stack { item: 565, count: 8, components: &[0, 0] } } },
    Recipe { id: "minecraft:yellow_wool_slab", kind: RecipeKind::Shaped { width: 3, height: 1, pattern: PATTERN_810, result: Stack { item: 286, count: 6, components: &[0, 0] } } },
    Recipe { id: "minecraft:yellow_wool_stairs", kind: RecipeKind::Shaped { width: 3, height: 3, pattern: PATTERN_811, result: Stack { item: 270, count: 4, components: &[0, 0] } } },
];
