/**
 * 热门模组「中文名 / 常见叫法 → Modrinth 英文搜索词」映射。
 * 用于让用户在搜索框输入中文也能搜到对应模组（Modrinth 标题为英文）。
 * 词条按热门度与常用社区叫法收录，可直接追加。
 */
const MOD_NAME_MAP: Array<[string, string]> = [
  // 性能 / 优化
  ['钠', 'Sodium'],
  ['锂', 'Lithium'],
  ['磷', 'Phosphor'],
  ['星光', 'Starlight'],
  ['模组菜单', 'Mod Menu'],
  ['布料配置', 'Cloth Config'],
  ['建筑', 'Architectury'],
  ['优化', 'Sodium'],
  ['高清修复', 'OptiFine'],

  // 基础功能 / 辅助
  ['小地图', "Xaero's Minimap"],
  ['旅行地图', 'JourneyMap'],
  ['大地图', "Xaero's World Map"],
  ['物品管理器', 'Just Enough Items'],
  ['物品栏', 'Roughly Enough Items'],
  ['苹果皮', 'AppleSkin'],
  ['玉', 'Jade'],
  ['一键整理', 'Inventory Sorter'],
  ['整理', 'Inventory Sorter'],
  ['鼠标增强', 'Mouse Tweaks'],
  ['按键控制', 'Controlling'],
  ['合并经验球', 'Clumps'],
  ['快速工作台', 'FastWorkbench'],
  ['快速熔炉', 'FastFurnace'],
  ['区块预生成', 'Chunky'],
  ['地平线', 'Distant Horizons'],
  ['光影', 'Iris'],

  // 储存 / 搬运
  ['储物抽屉', 'Storage Drawers'],
  ['铁箱子', 'Iron Chests'],
  ['精致存储', 'Refined Storage'],
  ['精致背包', 'Sophisticated Backpacks'],
  ['传送石碑', 'Waystones'],
  ['墓碑', 'Gravestone'],

  // 科技 / 能源
  ['机械动力', 'Create'],
  ['通用机械', 'Mekanism'],
  ['应用能源', 'Applied Energistics 2'],
  ['龙之研究', 'Draconic Evolution'],
  ['无尽贪婪', 'Avaritia'],

  // 魔法 / 探险
  ['植物魔法', 'Botania'],
  ['神秘时代', 'Thaumcraft'],
  ['星辉魔法', 'Astral Sorcery'],
  ['血魔法', 'Blood Magic'],
  ['匠魂', "Tinkers' Construct"],
  ['暮色森林', 'Twilight Forest'],
  ['天堂', 'Aether'],
  ['幸运方块', 'Lucky Block'],
  ['更多生物群系', "Biomes O' Plenty"],
  ['你将去的生物群系', "Oh The Biomes You'll Go"],
  ['宁静的季节', 'Serene Seasons'],
  ['更好的下界', 'Better Nether'],
  ['更好的末地', 'Better End'],

  // 世界 / 生物
  ['冰与火', 'Ice and Fire'],
  ['亚历克斯的怪物', "Alex's Mobs"],

  // 农业 / 生活
  ['农夫乐事', "Farmer's Delight"],
  ['潘马斯农场', "Pam's HarvestCraft"],
  ['丰收', 'Croptopia'],

  // 接口 / 拓展
  ['挂件', 'Trinkets'],
  ['饰品', 'Curios'],
  ['夸克', 'Quark'],
]

/**
 * 将搜索框里的中文/常见叫法翻译为 Modrinth 能搜到的英文词。
 * 命中任何词条就返回替换后的查询串；没命中保持原样（英语搜索照常工作）。
 */
export function translateModQuery(query: string): string {
  const q = query.trim()
  if (!q) return q
  let result = q
  let hit = false
  for (const [cn, en] of MOD_NAME_MAP) {
    if (result.includes(cn)) {
      result = result.replace(cn, en)
      hit = true
    }
  }
  return hit ? result : q
}

/** 反向：英文名 → 中文显示名（供界面展示，暂无使用可忽略） */
export function modChineseName(enName: string): string | undefined {
  const lower = enName.toLowerCase()
  for (const [cn, en] of MOD_NAME_MAP) {
    if (lower.includes(en.toLowerCase())) return cn
  }
  return undefined
}