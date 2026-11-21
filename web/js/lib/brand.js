/** Brand mark SVG (inline, uses CSS vars). */

export function brandMark(size = 28) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 32 32" fill="none" aria-hidden="true">
  <defs>
    <linearGradient id="dyncMarkG" x1="4" y1="4" x2="28" y2="28" gradientUnits="userSpaceOnUse">
      <stop stop-color="var(--accent)"/>
      <stop offset="1" stop-color="var(--accent-2)"/>
    </linearGradient>
  </defs>
  <path d="M 24.485 7.515 A 12 12 0 0 1 24.485 24.485" stroke="url(#dyncMarkG)" stroke-width="2.5" stroke-linecap="round"/>
  <path d="M 7.515 24.485 A 12 12 0 0 1 7.515 7.515" stroke="url(#dyncMarkG)" stroke-width="2.5" stroke-linecap="round"/>
  <polyline points="11,12 15,16 11,20" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/>
  <line x1="17" y1="20" x2="21" y2="20" stroke="currentColor" stroke-width="2.5" stroke-linecap="round"/>
</svg>`;
}

export function wordmark() {
  return `<span class="wordmark">Sync<span class="ai">Ai</span>Dync</span>`;
}
