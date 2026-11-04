import en from './en'

/**
 * 「颠倒英文」——把英文界面整串反转 + 逐字符翻转（ɥsᴉlƃuƎ → English 的经典倒置写法）。
 *
 * 不手写 500 多条翻转文案，而是从 en.ts 自动派生：英文改了，这里自动跟着变。
 * 两类内容必须原样保留，否则界面会坏：
 *   1. `{count}` 这类 i18n 占位符 —— 翻转后 vue-i18n 就找不到变量了
 *   2. `<a href="…">` 这类 HTML 标签 —— 翻转后浏览器就认不出标签了
 * 做法是先把它们换成单字符哨兵（私用区），整串反转后还原。
 */

/** 字符 → 倒置字形。没有映射的字符（中文、标点以外的符号）原样保留。 */
const FLIP_MAP: Record<string, string> = {
  a: 'ɐ', b: 'q', c: 'ɔ', d: 'p', e: 'ǝ', f: 'ɟ', g: 'ƃ', h: 'ɥ', i: 'ᴉ', j: 'ɾ',
  k: 'ʞ', l: 'l', m: 'ɯ', n: 'u', o: 'o', p: 'd', q: 'b', r: 'ɹ', s: 's', t: 'ʇ',
  u: 'n', v: 'ʌ', w: 'ʍ', x: 'x', y: 'ʎ', z: 'z',

  A: '∀', B: '𐐒', C: 'Ɔ', D: 'ᗡ', E: 'Ǝ', F: 'Ⅎ', G: '⅁', H: 'H', I: 'I', J: 'ſ',
  K: 'ʞ', L: '˥', M: 'W', N: 'N', O: 'O', P: 'Ԁ', Q: 'Ό', R: 'ᴚ', S: 'S', T: '⊥',
  U: '∩', V: 'Λ', W: 'M', X: 'X', Y: '⅄', Z: 'Z',

  '0': '0', '1': 'Ɩ', '2': 'ᄅ', '3': 'Ɛ', '4': 'ㄣ', '5': 'ϛ', '6': '9', '7': 'ㄥ', '8': '8', '9': '6',

  '.': '˙', ',': "'", '?': '¿', '!': '¡', '(': ')', ')': '(', '[': ']', ']': '[',
  '{': '}', '}': '{', '<': '>', '>': '<', '&': '⅋', _: '‾', '"': '„', "'": ',',
  ';': '؛', ':': 'ː', '/': '\\', '\\': '/',
}

/** 私用区哨兵起点（BMP 私用区 0xE000–0xF8FF，最多 6400 个，够用） */
const SENTINEL_BASE = 0xe000
/** 带内容的行内元素：整段保护，否则反转后 </a> 会跑到 <a> 前面，链接直接失效 */
const INLINE_ELEMENT = /<([a-zA-Z][\w-]*)\b[^>]*>([\s\S]*?)<\/\1>/g
/** 其余需要保护的片段：i18n 占位符 + 落单的标签（<br> 之类） */
const PROTECTED = /\{[a-zA-Z0-9_]+\}|<\/?[a-zA-Z][^>]*>/g

function flipGlyph(ch: string): string {
  return FLIP_MAP[ch] ?? ch
}

/** 把一段文字换成哨兵，原件存进 stash，等整行反转完再还原 */
function stashOne(stash: string[], value: string): string {
  stash.push(value)
  return String.fromCharCode(SENTINEL_BASE + stash.length - 1)
}

/** 单行翻转：保护片段 → 整串反转并逐字翻转 → 还原片段 */
function flipLine(line: string): string {
  const stash: string[] = []

  // 1) 先整段保护「开标签 + 内容 + 闭标签」，只把里面的文字翻转，标签顺序保持原样
  let masked = line.replace(INLINE_ELEMENT, (m, _tag, inner: string) => {
    const open = m.slice(0, m.indexOf('>') + 1)
    const close = m.slice(m.lastIndexOf('</'))
    return stashOne(stash, open + flipLine(inner) + close)
  })

  // 2) 再保护占位符与落单的标签
  masked = masked.replace(PROTECTED, (m) => stashOne(stash, m))
  // Array.from 按码位遍历，表情等代理对不会被劈成两半
  const flipped = Array.from(masked)
    .reverse()
    .map(flipGlyph)
    .join('')
  return flipped.replace(/[\uE000-\uF8FF]/g, (c) => stash[c.charCodeAt(0) - SENTINEL_BASE] ?? c)
}

/** 整段翻转；多行文案逐行处理，避免行序也被倒过来 */
export function flipText(input: string): string {
  if (!input) return input
  return input.split('\n').map(flipLine).join('\n')
}

type DeepStringMap<T> = { [K in keyof T]: T[K] extends string ? string : DeepStringMap<T[K]> }

/** 递归翻转整个语言字典（保持与 en 完全相同的键结构） */
function flipDeep<T>(value: T): DeepStringMap<T> {
  if (typeof value === 'string') {
    return flipText(value) as unknown as DeepStringMap<T>
  }
  const out: Record<string, unknown> = {}
  for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
    out[k] = flipDeep(v as unknown)
  }
  return out as DeepStringMap<T>
}

export const enFlip = flipDeep(en)

export default enFlip
