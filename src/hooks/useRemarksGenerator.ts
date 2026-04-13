import { useState, useEffect } from 'react';
import api from '@/lib/api';

export interface Statute {
  id: number;
  keyword: string;
  display_name: string;
  is_prohibited: boolean;
  supdt_goods_clause: string;
  adjn_goods_clause: string;
  legal_reference: string;
}

interface CaseContext {
  pax_name?: string;
  flight_no?: string;
  flight_date?: string;
  port_of_dep_dest?: string;
  os_date?: string;
  passport_no?: string;
  passport_date?: string;
  case_type?: string;
}

// Contextual answers provided by the user before generating remarks
export interface ContextualAnswers {
  cigarettes_has_cotpa_warning?: boolean;   // true = has COTPA warning, false = no warning (stricter language)
  currency_above_threshold?: boolean;       // true = above FEMA threshold
}

// Detected question that needs user input before generating
export interface ContextualQuestion {
  key: keyof ContextualAnswers;
  question: string;
  yesLabel?: string;
  noLabel?: string;
}

// ── UQC labels ──────────────────────────────────────────────────────────────
const UQC_LABEL: Record<string, string> = {
  NOS: 'Nos.', STK: 'Sticks', KGS: 'Kgs.', GMS: 'Gms.',
  LTR: 'Ltrs.', MTR: 'Mtrs.', PRS: 'Pairs',
};
const uqcLabel = (code: string) => UQC_LABEL[(code || '').toUpperCase()] ?? (code || 'Nos.');
const fmtQty = (q: number | string) => {
  const n = Number(q); return n % 1 === 0 ? Math.trunc(n).toString() : String(q);
};

// ── Helpers ─────────────────────────────────────────────────────────────────
function formatDate(d: string | undefined): string {
  if (!d) return '';
  const parts = d.split('-');
  if (parts.length === 3) return `${parts[2]}.${parts[1]}.${parts[0]}`;
  return d;
}

function joinNatural(parts: string[]): string {
  if (parts.length === 0) return '';
  if (parts.length === 1) return parts[0];
  return parts.slice(0, -1).join(', ') + ' and ' + parts[parts.length - 1];
}

/** Group items by description to merge quantities */
function mergedItemPhrases(items: any[]): string[] {
  const merged: Record<string, { qty: number; uqc: string; desc: string }> = {};
  for (const item of items) {
    const desc = (item.items_desc || 'goods').trim().toUpperCase();
    const key = desc + '|' + (item.items_uqc || 'NOS');
    if (merged[key]) {
      merged[key].qty += Number(item.items_qty || 1);
    } else {
      merged[key] = { qty: Number(item.items_qty || 1), uqc: item.items_uqc || 'NOS', desc };
    }
  }
  return Object.values(merged).map(m => `${fmtQty(m.qty)} ${uqcLabel(m.uqc)} of ${m.desc}`);
}

/** Detect which contextual questions are relevant for the current item list */
export function detectContextualQuestions(items: any[]): ContextualQuestion[] {
  const questions: ContextualQuestion[] = [];
  const hasCigarettes = items.some(i => {
    const desc = (i.items_desc || '').toLowerCase();
    const dtype = (i.items_duty_type || '').toLowerCase();
    // E-cigarettes / vapes are prohibited under PECA 2019, not COTPA 2003 —
    // exclude them so the COTPA pictorial-warning question is never asked for e-cigs.
    const isECig = desc.includes('e-cig') || desc.includes('ecig') || desc.includes('e cig') ||
                   desc.includes('vape') || desc.includes('vaping') || desc.includes('iqos') ||
                   desc.includes('juul') || desc.includes('e-liquid') || desc.includes('electronic cigarette');
    if (isECig) return false;
    return desc.includes('cigarette') || desc.includes('cigar') || desc.includes('bidi') ||
           desc.includes('beedi') || dtype.includes('cigarette');
  });
  if (hasCigarettes) {
    questions.push({
      key: 'cigarettes_has_cotpa_warning',
      question: 'Do the cigarettes / cigars bear the statutory pictorial health warning as prescribed under the Cigarettes and Other Tobacco Products Act (COTPA), 2003?',
      yesLabel: 'Yes — Warning present',
      noLabel: 'No — Warning absent / not found',
    });
  }
  return questions;
}

// ── Alias map: common spelling variations → canonical statute keyword ────────
const KEYWORD_ALIASES: [string, string][] = [
  // E-Cigarettes / Vapes → "e-cig" statute
  ['ecigarette', 'e-cig'], ['ecigarettes', 'e-cig'],
  ['e cigarette', 'e-cig'], ['e cigarettes', 'e-cig'],
  ['e-cigarette', 'e-cig'], ['e-cigarettes', 'e-cig'],
  ['electronic cigarette', 'e-cig'], ['ecig', 'e-cig'],
  ['e cig', 'e-cig'],
  ['vape', 'e-cig'], ['vapes', 'e-cig'], ['vaping', 'e-cig'],
  ['juul', 'e-cig'], ['iqos', 'e-cig'],
  ['e liquid', 'e-cig'], ['e-liquid', 'e-cig'],

  // Cigarettes / tobacco → "cigarette" statute
  ['cigarattes', 'cigarette'], ['cigaratte', 'cigarette'],
  ['gudang garam', 'cigarette'], ['dunhill', 'cigarette'],
  ['esse lights', 'cigarette'], ['esse gold', 'cigarette'],
  ['esse special', 'cigarette'], ['marlboro', 'cigarette'],
  ['benson', 'cigarette'], ['b&h', 'cigarette'],
  ['555 cigarette', 'cigarette'], ['gold flake', 'cigarette'],
  ['djarum', 'cigarette'], ['more cigarette', 'cigarette'],
  ['mond cigarette', 'cigarette'], ['davidoff cigarette', 'cigarette'],
  ['black cigarette', 'cigarette'], ['bidi', 'cigarette'],
  ['beedi', 'cigarette'], ['cigar', 'cigarette'],

  // Liquor / Alcohol → "liquor" statute
  ['alcohol', 'liquor'], ['alcoholic', 'liquor'],
  ['whisky', 'liquor'], ['whiskey', 'liquor'],
  ['brandy', 'liquor'], ['wine', 'liquor'], ['beer', 'liquor'],
  ['vodka', 'liquor'], ['rum', 'liquor'], ['gin', 'liquor'],
  ['scotch', 'liquor'], ['bourbon', 'liquor'], ['cognac', 'liquor'],
  ['champagne', 'liquor'], ['tequila', 'liquor'], ['arrack', 'liquor'],
  ['booze', 'liquor'], ['liquor bottle', 'liquor'],
  ['bardinet', 'liquor'], ['beehive', 'liquor'],
  ['chivas', 'liquor'], ['johnnie walker', 'liquor'],
  ['jack daniels', 'liquor'], ['jim beam', 'liquor'],
  ['teachers', 'liquor'], ['red label', 'liquor'],
  ['black label', 'liquor'], ['absolute vodka', 'liquor'],
  ['st remy', 'liquor'],

  // Gutkha variations → "gutkha" statute
  ['gutka', 'gutkha'], ['pan masala', 'gutkha'], ['paan masala', 'gutkha'],
  ['pan parag', 'gutkha'], ['vimal', 'gutkha'],

  // Refurbished electronics → "refurbish" statute
  ['laptop', 'refurbish'], ['laptops', 'refurbish'],
  ['old and used laptop', 'refurbish'], ['used laptop', 'refurbish'],
  ['refurbished', 'refurbish'], ['second hand', 'refurbish'],
  ['rf laptop', 'refurbish'], ['rf/used laptop', 'refurbish'],
  ['refurbished phone', 'refurbish'], ['refurbished mobile', 'refurbish'],
  ['refurbished iphone', 'refurbish'],

  // Gold variations → "gold" statute
  ['jewellery', 'gold'], ['jewelry', 'gold'], ['gold bar', 'gold'],
  ['gold biscuit', 'gold'], ['gold chain', 'gold'], ['gold bit', 'gold'],
  ['yellow metal', 'gold'], ['gold ingot', 'gold'], ['gold ring', 'gold'],
  ['gold bangle', 'gold'], ['gold jewel', 'gold'],

  // Drone variations → "drone" statute
  ['dji', 'drone'], ['nano drone', 'drone'], ['quadcopter', 'drone'],

  // Poppy → "poppy" statute
  ['poppy seed', 'poppy'], ['poppy seeds', 'poppy'], ['poppy husk', 'poppy'],

  // Currency → "currency" statute
  ['indian currency', 'currency'], ['foreign currency', 'currency'],

  // Toys → "toy" statute
  ['toys', 'toy'], ['hot wheels', 'toy'],

  // Narcotics → "narcotic" statute
  ['narcotics', 'narcotic'], ['ganja', 'narcotic'], ['hashish', 'narcotic'],
  ['charas', 'narcotic'], ['cocaine', 'narcotic'], ['heroin', 'narcotic'],
  ['mdma', 'narcotic'], ['opium', 'narcotic'],
];
KEYWORD_ALIASES.sort((a, b) => b[0].length - a[0].length);

/**
 * Word-boundary-aware substring check.
 * Single-word terms (e.g. "gin", "rum") require non-alpha boundaries so they
 * don't fire on "original", "engine", "drum", "instruments", etc.
 * Multi-word phrases are already specific enough — plain substring is fine.
 */
function includesWholeWord(text: string, term: string): boolean {
  if (term.includes(' ')) return text.includes(term);
  const escaped = term.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return new RegExp(`(?<![a-z])${escaped}(?![a-z])`).test(text);
}

/** Match an item description against statute keywords + aliases */
function matchStatute(item: any, statutes: Statute[]): Statute | null {
  if (!Array.isArray(statutes)) return null;

  const raw = ((item.items_desc || '') + ' ' + (item.items_duty_type || '')).toLowerCase();
  const desc = raw.replace(/-/g, ' ').replace(/\s+/g, ' ').trim();

  for (const [alias, canonicalKeyword] of KEYWORD_ALIASES) {
    if (includesWholeWord(desc, alias)) {
      const statute = statutes.find(s => s.keyword === canonicalKeyword);
      if (statute) return statute;
    }
  }

  const sorted = [...statutes]
    .filter(s => s && s.keyword && s.keyword !== 'generic_commercial')
    .sort((a, b) => (b.keyword?.length || 0) - (a.keyword?.length || 0));
  for (const s of sorted) {
    if (includesWholeWord(desc, s.keyword)) return s;
  }
  return null;
}

// ── Item categorization helpers ──────────────────────────────────────────────
function categorize(item: any): string {
  return (item.items_release_category || '').toUpperCase();
}
function isConfs(item: any) { return categorize(item) === 'CONFS'; }
function isRf(item: any) { const c = categorize(item); return c === 'RF' || c === 'UNDER OS' || c === ''; }
function isRef(item: any) { return categorize(item) === 'REF'; }
function isDuty(item: any) { const c = categorize(item); return c === 'UNDER DUTY' || c === 'DUTY'; }

// ────────────────────────────────────────────────────────────────────────────
// MAIN HOOK
// ── Hardcoded fallbacks (match admin_api.py _REMARKS_TPL_DEFAULTS) ───────────
// Used when the admin template fetch hasn't completed yet or returns nothing.
const _SUPDT_CLOSING_DEFAULTS: Record<string, string> = {
  remarks_supdt_import_confs_closing:
    'The aforesaid goods were not declared to the Customs authorities at the time of arrival as required under Section 77 of the Customs Act, 1962, and the pax was unable to produce any valid import authorization/permit for the same. The said goods being absolutely prohibited from import are liable for confiscation under Sections 111(d), 111(l), 111(m) and 111(o) of the Customs Act, 1962 read with Section 3(3) of the Foreign Trade (Development & Regulation) Act, 1992, for improper importation. The pax has also rendered himself/herself liable for penal action under Section 112(a) of the Customs Act, 1962. Put up for Adjudication please.',
  remarks_supdt_import_rf_closing:
    'The aforesaid goods were not declared to the Customs authorities at the time of arrival as required under Section 77 of the Customs Act, 1962. The said goods, being commercial in nature and non-bonafide baggage, are liable for confiscation under Sections 111(d), 111(l) and 111(m) of the Customs Act, 1962 read with Section 3(3) of the Foreign Trade (Development & Regulation) Act, 1992. The pax has also rendered himself/herself liable for penal action under Section 112 of the Customs Act, 1962. Put up for Adjudication please.',
  remarks_supdt_import_ref_closing:
    'The aforesaid goods were not declared to the Customs authorities at the time of arrival as required under Section 77 of the Customs Act, 1962, and the pax was unable to produce any valid import authorization/permit for the same. The said goods being restricted for import under the applicable DGFT notification/policy are liable for confiscation under Sections 111(d), 111(l), 111(m) and 111(o) of the Customs Act, 1962 read with Section 3(3) of the Foreign Trade (Development & Regulation) Act, 1992. The pax has also rendered himself/herself liable for penal action under Section 112(a) of the Customs Act, 1962. Put up for Adjudication please.',
  remarks_supdt_export_confs_closing:
    'The aforesaid goods were not declared to the Customs authorities at the time of departure as required under Section 40 of the Customs Act, 1962, and the pax was unable to produce any valid export authorization/permit. The said goods being absolutely prohibited from export are liable for confiscation under Section 113 of the Customs Act, 1962 read with Section 3(3) of the Foreign Trade (Development & Regulation) Act, 1992, for improper exportation. The pax has also rendered himself/herself liable for penal action under Section 114 of the Customs Act, 1962. Put up for Adjudication please.',
  remarks_supdt_export_rf_closing:
    'The aforesaid goods were not declared to the Customs authorities at the time of departure as required under Section 40 of the Customs Act, 1962. The said goods, being in violation of export regulations, are liable for confiscation under Section 113 of the Customs Act, 1962 read with Section 3(3) of the Foreign Trade (Development & Regulation) Act, 1992. The pax has also rendered himself/herself liable for penal action under Section 114 of the Customs Act, 1962. Put up for Adjudication please.',
};

const _AC_DISPOSAL_DEFAULTS: Record<string, string> = {
  remarks_ac_import_confs_disposal:
    '{items} being absolutely prohibited from import cannot be allowed to be redeemed and are hereby absolutely confiscated under Sections 111(d), 111(l), 111(m) and 111(o) of the Customs Act, 1962 read with Section 3(3) of the FT(D&R) Act, 1992.',
  remarks_ac_import_rf_disposal:
    '{items} are allowed to be redeemed on payment of the applicable Customs duty and Redemption Fine as imposed under Section 125 of the Customs Act, 1962.',
  remarks_ac_import_ref_disposal:
    '{items} are directed to be re-exported within the time stipulated by the Customs authorities, on payment of Re-export Fine as imposed under Section 125 of the Customs Act, 1962.',
  remarks_ac_export_confs_disposal:
    '{items} being absolutely prohibited from export cannot be allowed to be redeemed and are hereby absolutely confiscated under Section 113 of the Customs Act, 1962.',
  remarks_ac_export_rf_disposal:
    '{items} are allowed to be redeemed on payment of the applicable Redemption Fine as imposed under Section 125 of the Customs Act, 1962.',
};

// ── Module-level cache — statutes are seeded at startup and never change ─────
// Without this, every OffenceForm mount (new/edit/view) re-fetches the full
// statutes list from the backend, adding a round-trip before the wand button works.
let _statutesCache: Statute[] | null = null;
let _statutesFetch: Promise<Statute[]> | null = null;
let _remarksTemplateCache: Record<string, string> | null = null;

// ────────────────────────────────────────────────────────────────────────────
export function useRemarksGenerator() {
  const [statutes, setStatutes] = useState<Statute[]>(_statutesCache ?? []);
  const [loading, setLoading] = useState(_statutesCache === null);
  const [remarksTemplates, setRemarksTemplates] = useState<Record<string, string>>(_remarksTemplateCache ?? {});

  useEffect(() => {
    if (_statutesCache !== null) return; // already loaded for this session
    let mounted = true;

    // Promise lock: if a concurrent mount already started the fetch, reuse it
    if (!_statutesFetch) {
      let retryCount = 0;
      const MAX_RETRIES = 6;
      const RETRY_DELAY_MS = 2000;
      _statutesFetch = (async () => {
        while (retryCount <= MAX_RETRIES) {
          try {
            const res = await api.get('/statutes');
            if (Array.isArray(res.data) && res.data.length > 0) {
              _statutesCache = res.data;
              return res.data as Statute[];
            }
          } catch { /* server still starting, retry */ }
          retryCount++;
          if (retryCount <= MAX_RETRIES) {
            await new Promise(r => setTimeout(r, RETRY_DELAY_MS));
          }
        }
        return [] as Statute[];
      })();
    }

    _statutesFetch.then(data => {
      // If retries exhausted and got nothing, clear lock so next mount can retry
      if (data.length === 0) _statutesFetch = null;
      if (!mounted) return;
      if (data.length > 0) setStatutes(data);
      setLoading(false);
    });

    return () => { mounted = false; };
  }, []);

  // Fetch remarks opening templates (non-critical — hardcoded fallbacks used if absent)
  useEffect(() => {
    if (_remarksTemplateCache !== null) {
      setRemarksTemplates(_remarksTemplateCache);
      return;
    }
    let mounted = true;
    api.get('/admin/config/remarks-templates')
      .then(res => {
        const out: Record<string, string> = {};
        for (const [k, v] of Object.entries(res.data as Record<string, any>)) {
          out[k] = (v as any).value || '';
        }
        _remarksTemplateCache = out;
        if (mounted) setRemarksTemplates(out);
      })
      .catch(() => {}); // fallback to hardcoded defaults in buildSupdtRemark
    return () => { mounted = false; };
  }, []);

  const generateRemark = (
    role: 'SUPDT' | 'ADJN',
    items: any[],
    context?: CaseContext,
    answers?: ContextualAnswers
  ): string => {
    try {
      if (!items || items.length === 0) return '';
      if (!Array.isArray(statutes) || statutes.length === 0) {
        // Server may still be starting — return empty so the textarea stays blank
        return '';
      }

      const limit = role === 'SUPDT' ? 1500 : 3000;

      // Split items into categories
      const confsItems = items.filter(isConfs);
      const rfItems = items.filter(isRf);
      const refItems = items.filter(isRef);
      const dutyItems = items.filter(isDuty);
      const allDutyOnly = dutyItems.length === items.length && items.length > 0;

      // Match statutes per item
      const itemStatutes = new Map<number, Statute | null>();
      for (let i = 0; i < items.length; i++) {
        itemStatutes.set(i, matchStatute(items[i], statutes));
      }

      // Group by statute keyword for clause dedup
      const statuteGroups = new Map<string, { statute: Statute; items: any[]; indices: number[] }>();
      const unmatchedItems: { item: any; index: number }[] = [];
      for (let i = 0; i < items.length; i++) {
        const s = itemStatutes.get(i);
        if (s) {
          const key = s.keyword;
          if (!statuteGroups.has(key)) {
            statuteGroups.set(key, { statute: s, items: [], indices: [] });
          }
          statuteGroups.get(key)!.items.push(items[i]);
          statuteGroups.get(key)!.indices.push(i);
        } else {
          unmatchedItems.push({ item: items[i], index: i });
        }
      }
      if (unmatchedItems.length > 0) {
        const fallback = statutes.find(s => s.keyword === 'generic_commercial');
        if (fallback) {
          statuteGroups.set('generic_commercial', {
            statute: fallback,
            items: unmatchedItems.map(u => u.item),
            indices: unmatchedItems.map(u => u.index),
          });
        }
      }

      const ctx = answers || {};

      if (role === 'SUPDT') {
        const text = buildSupdtRemark(items, context, allDutyOnly, confsItems, rfItems, refItems, dutyItems, statuteGroups, ctx);
        return truncate(text, limit);
      }
      const text = buildAdjnRemark(items, context, allDutyOnly, confsItems, rfItems, refItems, dutyItems, statuteGroups, ctx);
      return truncate(text, limit);
    } catch (error: any) {
      console.error('Remark generation error:', error);
      return '';
    }
  };

  // ── SUPDT REMARK BUILDER ────────────────────────────────────────────────
  function buildSupdtRemark(
    items: any[], ctx: CaseContext | undefined, allDutyOnly: boolean,
    confsItems: any[], rfItems: any[], refItems: any[], _dutyItems: any[],
    statuteGroups: Map<string, { statute: Statute; items: any[]; indices: number[] }>,
    answers: ContextualAnswers,
  ): string {
    const parts: string[] = [];

    // 1. OPENING — arrival/departure + flight + passport
    const date = formatDate(ctx?.flight_date || ctx?.os_date);
    const city = ctx?.port_of_dep_dest || '';
    const flight = ctx?.flight_no || '';
    const ppNo = ctx?.passport_no || '';
    const isExport = (ctx?.case_type || '').trim().toUpperCase() === 'EXPORT CASE';

    // Template placeholder substitution helper
    const _applyTpl = (tpl: string) => tpl
      .replace(/\{date\}/g, date || 'the date of interception')
      .replace(/\{city\}/g, city || 'the destination')
      .replace(/\{flight_no\}/g, flight || 'the flight');

    let arrPart = '';
    if (isExport) {
      if (date || city || flight) {
        // Admin-configurable template (Legal Statutes → Remarks Templates in admin panel)
        const tpl = remarksTemplates['remarks_export_supdt_opening'] ||
          'On the basis of specific intelligence/information received by the department, the pax was detained at the Departure Hall on {date} while about to depart to {city} by flight no. {flight_no}.';
        arrPart = _applyTpl(tpl);
      } else {
        arrPart = 'The pax was detained at the Departure Hall while attempting to export goods without proper Customs declaration.';
      }
    } else {
      if (date && city && flight) {
        const tpl = remarksTemplates['remarks_import_supdt_opening'] ||
          'The pax arrived on {date} by flight no. {flight_no} from {city}.';
        arrPart = _applyTpl(tpl);
      } else if (date && flight) {
        arrPart = `The pax arrived on ${date} by flight no. ${flight}.`;
      } else {
        arrPart = 'The pax arrived and was intercepted at the Customs Examination Hall.';
      }
    }
    if (ppNo) {
      arrPart += ` The pax was holding Passport No. ${ppNo}`;
      if (ctx?.passport_date) {
        arrPart += ` (valid upto ${formatDate(ctx.passport_date)}).`;
      } else {
        arrPart += '.';
      }
    }
    parts.push(arrPart);

    // 2. ITEM DISCOVERY
    const allPhrases = mergedItemPhrases(items);
    if (allPhrases.length > 0) {
      parts.push(`On examination of the baggage, the following goods were found: ${joinNatural(allPhrases)}.`);
    }

    // 3. ALL DUTY ONLY — concise remark
    if (allDutyOnly) {
      const totalDutyVal = items.reduce((s: number, i: any) => {
        const val = Number(i.items_value || 0);
        const fa = computeEffFa(i, val);
        return s + Math.max(0, val - fa);
      }, 0);
      const totalDuty = items.reduce((s: number, i: any) => s + Number(i.items_duty || 0), 0);
      if (isExport) {
        parts.push(
          `The aforesaid goods were not declared to the Customs authorities at the time of departure as required under Section 40 of the Customs Act, 1962. ` +
          `The total assessable value amounts to Rs. ${Math.round(totalDutyVal).toLocaleString('en-IN')}/- on which customs duty of Rs. ${Math.round(totalDuty).toLocaleString('en-IN')}/- is payable. ` +
          `The said goods are therefore liable for confiscation under Section 113 of the Customs Act, 1962. Put up for Adjudication please.`
        );
      } else {
        parts.push(
          `The aforesaid goods are dutiable and were not declared to the Customs authorities at the time of arrival as mandated under Section 77 of the Customs Act, 1962. ` +
          `The total assessable value (after free allowance) amounts to Rs. ${Math.round(totalDutyVal).toLocaleString('en-IN')}/- on which customs duty of Rs. ${Math.round(totalDuty).toLocaleString('en-IN')}/- is payable. ` +
          `Since the said goods were concealed and not declared, they are also liable for confiscation under Section 111(m) of the Customs Act, 1962. Put up for Adjudication please.`
        );
      }
      return parts.join(' ');
    }

    // 4. PROHIBITED ITEMS — legal clause + contextual remarks
    const prohibitedGroups = [...statuteGroups.values()].filter(g => g.statute.is_prohibited);
    for (const grp of prohibitedGroups) {
      const relevant = grp.items.filter(i => !isDuty(i));
      if (relevant.length === 0) continue;

      // Cigarettes with COTPA-specific handling
      if (grp.statute.keyword === 'cigarette') {
        const cigPhrases = mergedItemPhrases(relevant);
        const hasCotpa = answers.cigarettes_has_cotpa_warning;
        if (hasCotpa === false) {
          // No COTPA warning — stricter language
          parts.push(
            `The ${joinNatural(cigPhrases)} were examined and found to be without the mandatory pictorial health warning as prescribed under Section 7 of the Cigarettes and Other Tobacco Products (Prohibition of Advertisement and Regulation of Trade and Commerce, Production, Supply and Distribution) Act, 2003 (COTPA). ` +
            `The import of cigarettes/tobacco products without such statutory warning is strictly prohibited in terms of the COTPA Rules, 2004 read with DGFT policy. ` +
            `The said goods are therefore absolutely prohibited and are liable for confiscation.`
          );
        } else {
          // Has COTPA warning or unknown — standard commercial quantity language
          parts.push(grp.statute.supdt_goods_clause);
          parts.push(`The quantity of ${joinNatural(cigPhrases)} found is in commercial quantity and far exceeds the permissible free allowance of 100 Sticks under the Baggage Rules, 2016, and hence the same cannot be considered as bonafide baggage.`);
        }
      } else {
        parts.push(grp.statute.supdt_goods_clause);
        if (grp.statute.keyword !== 'narcotic' && grp.statute.keyword !== 'poppy') {
          parts.push(isExport
            ? `The said goods are being attempted to be exported without proper export authorization/permit.`
            : `The said goods are in commercial quantity and hence cannot be considered as bonafide baggage.`
          );
        }
      }
    }

    // 5. OVER-ALLOWANCE / DUTIABLE ITEMS — FA deduction detail
    const nonProhibitedGroups = [...statuteGroups.values()].filter(g => !g.statute.is_prohibited);
    for (const grp of nonProhibitedGroups) {
      const relevant = grp.items.filter(i => !isDuty(i));
      if (relevant.length === 0 && grp.items.some(isDuty)) continue;

      const allGroupItems = grp.items;
      const totalQty = allGroupItems.reduce((s: number, i: any) => s + Number(i.items_qty || 1), 0);
      const totalFaQty = allGroupItems.reduce((s: number, i: any) => s + Number(i.items_fa_qty || 0), 0);
      const hasFaQty = totalFaQty > 0 && allGroupItems.some(i => (i.items_fa_type || 'value') === 'qty');
      const totalFaVal = allGroupItems.reduce((s: number, i: any) => s + Number(i.items_fa || 0), 0);
      const desc = (allGroupItems[0].items_desc || 'goods').trim();
      const uqc = uqcLabel(allGroupItems[0].items_uqc || 'NOS');
      const totalVal = allGroupItems.reduce((s: number, i: any) => s + Number(i.items_value || 0), 0);

      if (hasFaQty && totalFaQty > 0) {
        const excess = Math.max(0, totalQty - totalFaQty);
        parts.push(
          `Out of the total ${fmtQty(totalQty)} ${uqc} of ${desc} brought by the pax, a free allowance of ${fmtQty(totalFaQty)} ${uqc} is admissible under the Baggage Rules, 2016. ` +
          (excess > 0
            ? `The remaining ${fmtQty(excess)} ${uqc} of ${desc}, valued at approximately Rs. ${Math.round(totalVal * excess / totalQty).toLocaleString('en-IN')}/-, exceeds the permissible free allowance and is commercial in nature and non-bonafide baggage.`
            : ``)
        );
      } else if (totalFaVal > 0) {
        const excessVal = Math.max(0, totalVal - totalFaVal);
        parts.push(
          `Out of the total value of Rs. ${Math.round(totalVal).toLocaleString('en-IN')}/- of ${desc} brought by the pax, a free allowance of Rs. ${Math.round(totalFaVal).toLocaleString('en-IN')}/- is admissible under the Baggage Rules, 2016. ` +
          (excessVal > 0
            ? `The remaining value of Rs. ${Math.round(excessVal).toLocaleString('en-IN')}/- exceeds the permissible free allowance and is therefore dutiable.`
            : `The goods fall entirely within the free allowance.`)
        );
      } else if (relevant.length > 0) {
        // No FA at all — commercial goods
        parts.push(grp.statute.supdt_goods_clause);
        parts.push(`The said goods are commercial in nature and not bonafide baggage.`);
      }
    }

    // 6. CLOSING — category-determined template (indirect language; section numbers
    //    are the signal — SUPDT does NOT recommend confiscation/redemption explicitly)
    const hasConfs = confsItems.length > 0;
    const hasRf    = rfItems.length > 0;
    const hasRef   = refItems.length > 0;

    // Pick which closing template applies based on dominant category:
    //   CONFS-only          → prohibited-goods language  (Sec 111(o) + "improper importation")
    //   REF-only            → restricted-goods language  (Sec 111(o) but no "improper")
    //   RF or mixed         → commercial-goods language  (Sec 111(d)(l)(m) only, no (o))
    //   Export CONFS-only   → Sec 113 + "improper exportation"
    //   Export RF/mixed     → Sec 113, commercial language
    let closingKey: string;
    if (isExport) {
      closingKey = (hasConfs && !hasRf)
        ? 'remarks_supdt_export_confs_closing'
        : 'remarks_supdt_export_rf_closing';
    } else if (hasConfs && !hasRf && !hasRef) {
      closingKey = 'remarks_supdt_import_confs_closing';
    } else if (hasRef && !hasConfs && !hasRf) {
      closingKey = 'remarks_supdt_import_ref_closing';
    } else {
      closingKey = 'remarks_supdt_import_rf_closing';
    }
    parts.push(remarksTemplates[closingKey] || _SUPDT_CLOSING_DEFAULTS[closingKey]);

    return parts.join(' ');
  }

  // ── AC REMARK BUILDER ───────────────────────────────────────────────────
  function buildAdjnRemark(
    items: any[], ctx: CaseContext | undefined, allDutyOnly: boolean,
    confsItems: any[], rfItems: any[], refItems: any[], dutyItems: any[],
    statuteGroups: Map<string, { statute: Statute; items: any[]; indices: number[] }>,
    answers: ContextualAnswers,
  ): string {
    const parts: string[] = [];
    const isExport = (ctx?.case_type || '').trim().toUpperCase() === 'EXPORT CASE';

    // 1. OPENING — hearing statement
    parts.push(
      'Heard the pax. The pax was granted waiver of Show Cause Notice as requested. ' +
      'Translated vernacularly as understood.'
    );

    // 2. PAX STATEMENT — what pax claimed
    const allPhrases = mergedItemPhrases(items);
    if (allPhrases.length > 0) {
      parts.push(isExport
        ? `The pax stated that the ${joinNatural(allPhrases)} were being carried for personal use and were not meant for commercial purposes.`
        : `The pax stated that the ${joinNatural(allPhrases)} were brought by him/her for personal use and were not meant for commercial purposes.`
      );
    }

    // 3. DUTY-ONLY — concise finding
    if (allDutyOnly) {
      const totalDutyVal = dutyItems.reduce((s: number, i: any) => {
        const val = Number(i.items_value || 0);
        const fa = computeEffFa(i, val);
        return s + Math.max(0, val - fa);
      }, 0);
      const totalDuty = dutyItems.reduce((s: number, i: any) => s + Number(i.items_duty || 0), 0);
      if (isExport) {
        parts.push(
          `On examination, the goods were found to be liable for export restrictions and were not declared to Customs as required under Section 40 of the Customs Act, 1962. ` +
          `The total assessable value of the goods is Rs. ${Math.round(totalDutyVal).toLocaleString('en-IN')}/-.`
        );
        parts.push(
          `The said goods are therefore liable for confiscation under Section 113 of the Customs Act, 1962 for non-declaration. ` +
          `The goods are, however, being allowed to be redeemed on payment of the applicable redemption fine as imposed. ` +
          `A personal penalty is also imposed under Section 114(i) of the Customs Act, 1962.`
        );
      } else {
        parts.push(
          `On examination, the goods were found to be dutiable and were not declared to Customs as required under Section 77 of the Customs Act, 1962. ` +
          `After deducting the permissible free allowance under the Baggage Rules, 2016, the total assessable value works out to Rs. ${Math.round(totalDutyVal).toLocaleString('en-IN')}/- on which the applicable Customs Duty is Rs. ${Math.round(totalDuty).toLocaleString('en-IN')}/-.`
        );
        parts.push(
          `The said goods are therefore liable for confiscation under Section 111(m) of the Customs Act, 1962 for non-declaration. ` +
          `The goods are, however, being allowed to be redeemed on payment of the applicable customs duty and redemption fine as imposed. ` +
          `A personal penalty is also imposed under Section 112(b) of the Customs Act, 1962.`
        );
      }
      return parts.join(' ');
    }

    // 4. LEGAL FINDINGS — for each prohibited item group
    const prohibitedGroups = [...statuteGroups.values()].filter(g => g.statute.is_prohibited);
    for (const grp of prohibitedGroups) {
      const relevant = grp.items.filter(i => !isDuty(i));
      if (relevant.length === 0) continue;
      const itemPhrases = mergedItemPhrases(relevant);

      if (grp.statute.keyword === 'cigarette') {
        const hasCotpa = answers.cigarettes_has_cotpa_warning;
        if (hasCotpa === false) {
          parts.push(
            `The ${joinNatural(itemPhrases)} found were examined and were without the mandatory statutory pictorial health warning as prescribed under Section 7 of the COTPA Act, 2003. ` +
            `The import of cigarettes/tobacco products not bearing the statutory warning is absolutely prohibited in terms of the COTPA Rules, 2004. ` +
            `The pax's claim of personal use is not accepted as the goods are in excess of reasonable personal consumption and are absolutely prohibited from import in this form. ` +
            `No free allowance is admissible on goods that are absolutely prohibited.`
          );
        } else {
          parts.push(grp.statute.adjn_goods_clause);
          // Check if there's excess beyond FA
          const totalQty = relevant.reduce((s: number, i: any) => s + Number(i.items_qty || 1), 0);
          const totalFaQty = relevant.reduce((s: number, i: any) => s + Number(i.items_fa_qty || 0), 0);
          if (totalFaQty > 0 && totalQty > totalFaQty) {
            const excess = totalQty - totalFaQty;
            const uqc = uqcLabel(relevant[0].items_uqc || 'STK');
            parts.push(
              `Out of the total ${fmtQty(totalQty)} ${uqc}, a free allowance of ${fmtQty(totalFaQty)} ${uqc} is permissible under the Baggage Rules, 2016. ` +
              `The remaining ${fmtQty(excess)} ${uqc} of ${grp.items[0].items_desc || 'Cigarettes'} beyond the free allowance are not bonafide baggage and are in commercial quantity.`
            );
          }
        }
      } else {
        parts.push(grp.statute.adjn_goods_clause);
      }
    }

    // 5. NON-PROHIBITED OVER-ALLOWANCE ITEMS — FA detail in AC findings
    const nonProhibitedGroups = [...statuteGroups.values()].filter(g => !g.statute.is_prohibited);
    for (const grp of nonProhibitedGroups) {
      const relevant = grp.items.filter(i => !isDuty(i));
      if (relevant.length === 0 && grp.items.some(isDuty)) continue;

      const allGroupItems = grp.items;
      const totalQty = allGroupItems.reduce((s: number, i: any) => s + Number(i.items_qty || 1), 0);
      const totalFaQty = allGroupItems.reduce((s: number, i: any) => s + Number(i.items_fa_qty || 0), 0);
      const hasFaQty = totalFaQty > 0 && allGroupItems.some(i => (i.items_fa_type || 'value') === 'qty');
      const totalFaVal = allGroupItems.reduce((s: number, i: any) => s + Number(i.items_fa || 0), 0);
      const desc = (allGroupItems[0].items_desc || 'goods').trim();
      const uqc = uqcLabel(allGroupItems[0].items_uqc || 'NOS');
      const totalVal = allGroupItems.reduce((s: number, i: any) => s + Number(i.items_value || 0), 0);

      if (hasFaQty && totalFaQty > 0) {
        const excess = Math.max(0, totalQty - totalFaQty);
        parts.push(
          `Out of the total ${fmtQty(totalQty)} ${uqc} of ${desc} found in the baggage, a free allowance of ${fmtQty(totalFaQty)} ${uqc} is admissible under the Baggage Rules, 2016. ` +
          (excess > 0
            ? `The remaining ${fmtQty(excess)} ${uqc} of ${desc}, valued at Rs. ${Math.round(totalVal * excess / totalQty).toLocaleString('en-IN')}/-, is in excess of the permissible limit and is non-bonafide in nature.`
            : '')
        );
      } else if (totalFaVal > 0) {
        const excessVal = Math.max(0, totalVal - totalFaVal);
        parts.push(
          `Out of the total value of Rs. ${Math.round(totalVal).toLocaleString('en-IN')}/- of ${desc}, a free allowance of Rs. ${Math.round(totalFaVal).toLocaleString('en-IN')}/- is admissible under the Baggage Rules, 2016. ` +
          (excessVal > 0
            ? `The balance value of Rs. ${Math.round(excessVal).toLocaleString('en-IN')}/- is dutiable.`
            : `The goods are fully within the free allowance.`)
        );
      } else if (relevant.length > 0) {
        parts.push(grp.statute.adjn_goods_clause);
      }
    }

    // 6. DISPOSAL — per category
    const confsAbsPhrases = mergedItemPhrases(confsItems);
    const rfPhrases = mergedItemPhrases(rfItems.filter(i => !isDuty(i)));
    const refPhrases = mergedItemPhrases(refItems);
    const dutyPhrases = mergedItemPhrases(dutyItems);

    // Confiscation liability statement — section references depend on category.
    // CONFS/REF → include Section 111(o) + 112(a); RF-only → 111(d)(l)(m) + 112 only.
    const hasConfsOrRef = confsItems.length > 0 || refItems.length > 0;
    if (isExport) {
      parts.push(
        `The aforesaid goods are therefore liable for confiscation under Section 113 of the Customs Act, 1962 read with Section 3(3) of the Foreign Trade (Development & Regulation) Act, 1992, and the pax has rendered himself/herself liable for penal action under Section 114 of the Customs Act, 1962.`
      );
    } else if (hasConfsOrRef) {
      parts.push(
        `The aforesaid goods are therefore liable for confiscation under Sections 111(d), 111(l), 111(m) and 111(o) of the Customs Act, 1962 read with Section 3(3) of the Foreign Trade (Development & Regulation) Act, 1992, and the pax has rendered himself/herself liable for penal action under Section 112(a) of the Customs Act, 1962.`
      );
    } else {
      parts.push(
        `The aforesaid goods are therefore liable for confiscation under Sections 111(d), 111(l) and 111(m) of the Customs Act, 1962 read with Section 3(3) of the Foreign Trade (Development & Regulation) Act, 1992, and the pax has rendered himself/herself liable for penal action under Section 112 of the Customs Act, 1962.`
      );
    }

    // Helper: apply an AC disposal template (supports {items} placeholder)
    const _applyDisposal = (key: string, itemPhrases: string[]): string => {
      const tpl = remarksTemplates[key] || _AC_DISPOSAL_DEFAULTS[key] || '';
      return tpl.replace('{items}', `The ${joinNatural(itemPhrases)}`);
    };

    // Disposal orders — one sentence per category, only if items exist
    if (confsAbsPhrases.length > 0) {
      parts.push(_applyDisposal(
        isExport ? 'remarks_ac_export_confs_disposal' : 'remarks_ac_import_confs_disposal',
        confsAbsPhrases,
      ));
    }
    if (rfPhrases.length > 0) {
      parts.push(_applyDisposal(
        isExport ? 'remarks_ac_export_rf_disposal' : 'remarks_ac_import_rf_disposal',
        rfPhrases,
      ));
    }
    if (refPhrases.length > 0) {
      parts.push(_applyDisposal('remarks_ac_import_ref_disposal', refPhrases));
    }
    if (dutyPhrases.length > 0) {
      parts.push(
        `The ${joinNatural(dutyPhrases)} are allowed to be cleared on payment of the applicable Customs duty as assessed.`
      );
    }

    return parts.join(' ');
  }

  function truncate(text: string, limit: number): string {
    text = text.trim();
    if (text.length <= limit) return text;

    // Try smart truncation: cut at a sentence boundary near limit
    const cutAt = limit - 20;
    const lastDot = text.lastIndexOf('.', cutAt);
    if (lastDot > cutAt * 0.7) {
      return text.substring(0, lastDot + 1).trim();
    }
    return text.substring(0, cutAt - 3).trim() + '...';
  }

  return { statutes, loading, generateRemark };
}

// ── Utility used by both remark builders ────────────────────────────────────
function computeEffFa(item: any, itemValue: number): number {
  const cat = (item.items_release_category || '').toUpperCase();
  if (!['UNDER DUTY', 'UNDER OS', 'RF', 'REF'].includes(cat)) return 0;
  if ((item.items_fa_type || 'value') === 'qty') {
    const tq = Number(item.items_qty || 0);
    const fq = Number(item.items_fa_qty || 0);
    return tq > 0 ? Math.min((fq / tq) * itemValue, itemValue) : 0;
  }
  return Math.min(Number(item.items_fa || 0), itemValue);
}
