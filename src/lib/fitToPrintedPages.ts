/**
 * Keep the OS print to exactly two sheets, whatever the case contains.
 *
 * THE PROBLEM
 * The OS is designed as two pages: the booking report, then the adjudication
 * order. Page one ends with `page-break-after: always`, so page two always
 * starts on a fresh sheet — but nothing limits how tall page one may GROW. A
 * case with many seized items, or long remarks, makes it taller than one sheet,
 * the browser spills the remainder onto a second sheet, and the adjudication
 * order is pushed to a third. Officers hit this when the PDF download fails and
 * they fall back to Ctrl+P.
 *
 * WHY NOT JUST CLIP IT
 * `overflow: hidden` would guarantee two sheets and is the obvious fix. It is
 * also the wrong one: this is a seizure order, and clipping silently drops item
 * rows off the bottom of a legal document. Nobody notices at the printer — they
 * notice when the filed copy is short of an item. Losing content is a worse
 * failure than an extra sheet, so the content is SHRUNK to fit instead. Every
 * row still prints; it prints slightly smaller.
 *
 * HOW
 * On `beforeprint`, each page is measured and given a `zoom` factor if it is
 * over the printable height. `zoom` is used rather than `transform: scale()`
 * because it reflows the layout — a transform only scales the pixels while the
 * element still occupies its original height, so the overflow, and the third
 * sheet, would remain. Reset on `afterprint` so the screen is untouched.
 *
 * There is a floor on how far it will shrink. Past that the print is unreadable
 * and a third sheet is the better outcome, so it stops and lets the browser
 * paginate — an honest overflow rather than an illegible page.
 */

/** Legal sheet, less the margins declared in the print stylesheet. */
const SHEET_HEIGHT_IN = 14;
const MARGIN_TOP_IN = 0.35;
const MARGIN_BOTTOM_IN = 0.3;
const CSS_PX_PER_IN = 96;

/**
 * Aim slightly under the true printable height. The on-screen box uses `p-8`
 * while print uses `px-6 py-4`, so a measurement taken on screen is close to,
 * but not exactly, the printed height. The margin of error is small; this
 * absorbs it rather than landing a millimetre over and costing a whole sheet.
 */
const SAFETY = 0.97;

/** Below this the text stops being readable; an extra sheet is better. */
const MIN_ZOOM = 0.62;

const PRINTABLE_PX =
  (SHEET_HEIGHT_IN - MARGIN_TOP_IN - MARGIN_BOTTOM_IN) * CSS_PX_PER_IN * SAFETY;

export const PRINT_PAGE_CLASS = 'os-print-page';

type Zoomable = HTMLElement & { style: CSSStyleDeclaration };

function pages(): Zoomable[] {
  return Array.from(
    document.querySelectorAll<HTMLElement>(`.${PRINT_PAGE_CLASS}`)
  ) as Zoomable[];
}

function shrinkToFit(): void {
  for (const page of pages()) {
    page.style.zoom = '';               // measure unscaled
    const height = page.scrollHeight;
    if (height <= PRINTABLE_PX) continue;

    const needed = PRINTABLE_PX / height;
    if (needed < MIN_ZOOM) {
      // Too much content to shrink honestly. Leave it alone and let the browser
      // paginate — a readable third sheet beats an unreadable second one.
      continue;
    }
    page.style.zoom = String(Math.floor(needed * 1000) / 1000);
  }
}

function restore(): void {
  for (const page of pages()) page.style.zoom = '';
}

/**
 * Start keeping the print to two sheets. Returns a cleanup function, so a React
 * effect can hand it straight back.
 */
export function fitToPrintedPages(): () => void {
  window.addEventListener('beforeprint', shrinkToFit);
  window.addEventListener('afterprint', restore);

  // Safari and some WebViews do not fire beforeprint; the media-query listener
  // does. Both are registered because firing twice is harmless — the first call
  // clears its own zoom before measuring.
  const mql = window.matchMedia?.('print');
  const onMedia = (e: MediaQueryListEvent) => (e.matches ? shrinkToFit() : restore());
  mql?.addEventListener?.('change', onMedia);

  return () => {
    window.removeEventListener('beforeprint', shrinkToFit);
    window.removeEventListener('afterprint', restore);
    mql?.removeEventListener?.('change', onMedia);
    restore();
  };
}
