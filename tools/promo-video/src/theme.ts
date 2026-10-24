// Extracted from the published beta-0.4.0 shadcn theme; blue is film annotation only.
export const theme = {
  background: 'var(--film-background, #141416)', foreground: 'var(--film-foreground, #f4f4f5)',
  card: 'var(--film-card, #1c1c1f)', popover: 'var(--film-popover, #242427)',
  secondary: 'var(--film-secondary, #303034)', muted: 'var(--film-muted, #252528)',
  mutedForeground: 'var(--film-muted-foreground, #b1b1bb)',
  accent: 'var(--film-accent, #34343a)', border: 'var(--film-border, #3b3b42)', blue: '#91b6dc', blueDark: '#3d668d',
  stage: '#101318', stageLight: '#e9ecee', green: '#a9cdb9',
  font: '"Segoe UI", "Microsoft YaHei UI", "Microsoft YaHei", sans-serif',
  radius: 6, safeX: 96, safeY: 70,
} as const;
export const release = {version: '0.4.0', tag: 'beta-0.4.0',
  url: 'https://github.com/LitematicaViewer/Litematica-Nova/releases/tag/beta-0.4.0',
  source: '3cd63', publicMain: '8a0ee8cd'};
