// Global declarations for tsc --noEmit typecheck: names the page provides
// outside the ES modules. STATIC_DATA is declared per page, in
// `arc_page.d.ts` and `hotspot_page.d.ts`.

// Runtime placeholders (replaced by Rust at render time)
declare const __ROW_HEIGHT__: number;
declare const __MARGIN__: number;
declare const __TOOLBAR_HEIGHT__: number;
declare const __SIDEBAR_SHADOW_PAD__: number;
declare const __OVERLAYS__: 'svg' | 'page';

// Window augmentation
interface Window {
  DEBUG_ARCS?: boolean;
}
