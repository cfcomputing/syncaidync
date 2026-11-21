/** Inline SVG icons drawn for SyncAiDync (24×24, stroke currentColor). */

const attrs =
  'xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"';

function svg(body) {
  return `<svg ${attrs}>${body}</svg>`;
}

export const icons = {
  plus: svg('<path d="M12 5v14M5 12h14"/>'),
  sun: svg(
    '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M4.93 19.07l1.41-1.41M17.66 6.34l1.41-1.41"/>',
  ),
  moon: svg('<path d="M20 14.5A8.5 8.5 0 0 1 9.5 4 7 7 0 1 0 20 14.5z"/>'),
  auto: svg(
    '<circle cx="12" cy="12" r="8"/><path d="M12 4a8 8 0 0 1 0 16" fill="currentColor" stroke="none"/>',
  ),
  gear: svg(
    '<circle cx="12" cy="12" r="3"/><path d="M12 3.5v2.2M12 18.3v2.2M4.8 6.5l1.6 1.6M17.6 15.9l1.6 1.6M3.5 12h2.2M18.3 12h2.2M4.8 17.5l1.6-1.6M17.6 8.1l1.6-1.6"/>',
  ),
  grid: svg(
    '<rect x="4" y="4" width="7" height="7" rx="1"/><rect x="13" y="4" width="7" height="7" rx="1"/><rect x="4" y="13" width="7" height="7" rx="1"/><rect x="13" y="13" width="7" height="7" rx="1"/>',
  ),
  rows: svg(
    '<path d="M4 7h16M4 12h16M4 17h16"/>',
  ),
  free: svg(
    '<rect x="3.5" y="5" width="11" height="8" rx="1.5"/><rect x="9.5" y="11" width="11" height="8" rx="1.5"/>',
  ),
  "chevron-down": svg('<path d="M6 9l6 6 6-6"/>'),
  "chevron-right": svg('<path d="M9 6l6 6-6 6"/>'),
  dots: svg('<circle cx="6" cy="12" r="1.1" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="1.1" fill="currentColor" stroke="none"/><circle cx="18" cy="12" r="1.1" fill="currentColor" stroke="none"/>'),
  play: svg('<path d="M8 6.5v11l9-5.5z"/>'),
  stop: svg('<rect x="7" y="7" width="10" height="10" rx="1.5"/>'),
  trash: svg(
    '<path d="M5 8h14M10 8V6.5h4V8M9 8l.6 10h4.8L15 8"/>',
  ),
  external: svg('<path d="M10 5H6.5A1.5 1.5 0 0 0 5 6.5v11A1.5 1.5 0 0 0 6.5 19h11a1.5 1.5 0 0 0 1.5-1.5V14M13 5h6v6M11 13l8-8"/>'),
  back: svg('<path d="M14 6l-6 6 6 6M8 12h11"/>'),
  bell: svg('<path d="M7 17h10M8.5 17a3.5 3.5 0 0 1 7 0M6.5 10.5a5.5 5.5 0 0 1 11 0c0 3.2 1 4.5 1 4.5H5.5s1-1.3 1-4.5"/>'),
  snooze: svg(
    '<path d="M18.5 13.2A7 7 0 1 1 10.8 5.5"/><path d="M14.5 4.5h4l-4 4h4"/>',
  ),
  file: svg('<path d="M8 4.5h5.5L17.5 8.5V19a1 1 0 0 1-1 1H8a1 1 0 0 1-1-1V5.5a1 1 0 0 1 1-1z"/><path d="M13.5 4.5V8.5h4"/>'),
  folder: svg('<path d="M4 8.5A1.5 1.5 0 0 1 5.5 7H9l1.5 2H18.5A1.5 1.5 0 0 1 20 10.5v6A1.5 1.5 0 0 1 18.5 18h-13A1.5 1.5 0 0 1 4 16.5v-8z"/>'),
  diff: svg('<path d="M8 5v14M16 5v14M5 9h6M13 15h6"/>'),
  terminal: svg('<rect x="3.5" y="5" width="17" height="14" rx="2"/><path d="M7 10l3 2-3 2M12 14h5"/>'),
  info: svg('<circle cx="12" cy="12" r="8"/><path d="M12 11v5M12 8h.01"/>'),
  search: svg('<circle cx="11" cy="11" r="6"/><path d="M16 16l3.5 3.5"/>'),
  check: svg('<path d="M5 12.5l4.5 4.5L19 7.5"/>'),
  close: svg('<path d="M7 7l10 10M17 7L7 17"/>'),
  "pull-request": svg(
    '<circle cx="7" cy="6" r="2"/><circle cx="7" cy="18" r="2"/><circle cx="17" cy="18" r="2"/><path d="M7 8v8M17 16V9.5A2.5 2.5 0 0 0 14.5 7H12"/><path d="M12 4.5v5"/>',
  ),
  github: svg(
    '<path d="M8 19c-3 0-3-2-4-2.5M9 21v-3.5c0-1 .4-1.6 1-2-4-.4-6-2.1-6-5.3 0-1.3.5-2.4 1.4-3.3-.3-1-.2-2.4.2-3.4 1.4 0 2.7.8 3.5 1.4a13 13 0 0 1 5.8 0c.8-.6 2.1-1.4 3.5-1.4.4 1 .5 2.4.2 3.4.9.9 1.4 2 1.4 3.3 0 3.2-2 4.9-6 5.3.6.4 1 1 1 2V21"/>',
  ),
  branch: svg(
    '<circle cx="7" cy="5" r="2"/><circle cx="7" cy="19" r="2"/><circle cx="17" cy="6" r="2"/><path d="M7 7v10m10-9c0 6-10 2-10 9"/>',
  ),
  import: svg('<path d="M12 4v10M8.5 10.5 12 14l3.5-3.5M5 18h14"/>'),
};

export function icon(name) {
  return icons[name] || "";
}
