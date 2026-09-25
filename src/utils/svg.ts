export const cleanSvg = (svg: string) =>
  svg
    .replace(/width="[^"]*"/g, '')
    .replace(/height="[^"]*"/g, '')
    .replace(/<svg /, '<svg width="100%" height="100%" ')