import {
  useState, useCallback, useRef, useEffect, useMemo,
} from 'react';
import {
  Plus, Trash2, Download, MessageSquare, Settings2,
  Save, ChevronDown, ChevronRight, AlertTriangle, X,
} from 'lucide-react';
import api from '@/lib/api';
import { listOf } from '@/lib/listOf';
import ItemCombobox from './ItemCombobox';
import {
  DrEntry, DrDrEntry, DrOsEntry, DrSession, DrTariff, DrFormulaRule,
  blankEntry, blankDrEntry, blankOsEntry,
  computeDuties, computeTotal, getAutoFields, fmtDate,
} from './revenueCalc';
import { downloadAdcExcel, downloadRevenueExcel } from './dcrExcel';

interface ItemType { id: number; name: string; usage_count: number; is_system: boolean; }

interface Props {
  session: DrSession;
  rules: DrFormulaRule[];
  onMessage: () => void;
  onConfig: () => void;   // navigate to Rates & Formulas config page
  /** A formula was rewritten from the sheet; reload the rules for this shift. */
  onRulesChanged?: () => void;
}

// Column definitions for the main entry table
const ENTRY_COLS = [
  { key: 'sl_no',            label: 'SR',        width: 40,  type: 'int',    manual: false },
  { key: 'br_no',            label: 'BR No.',     width: 80,  type: 'text',   manual: true  },
  { key: 'is_offline_br',   label: 'Offl.',      width: 38,  type: 'bool',   manual: true  },
  { key: 'os_ref',           label: 'OS No.',     width: 90,  type: 'text',   manual: true  },
  { key: 'item_desc',        label: 'Item',       width: 130, type: 'combo',  manual: true  },
  { key: 'dutiable_value',   label: 'Value',      width: 80,  type: 'num',    manual: true  },
  { key: 'gold_weight_gms',  label: 'Gold(g)',    width: 60,  type: 'num',    manual: true  },
  { key: 'baggage_duty',     label: 'Bagg.Duty',  width: 80,  type: 'num',    manual: false },
  { key: 'liquor_duty',      label: 'Lqr.Duty',   width: 75,  type: 'num',    manual: false },
  { key: 'cigarette_duty',   label: 'Cig.Duty',   width: 70,  type: 'num',    manual: true  },
  { key: 'sw_sc',            label: 'SW SC',      width: 70,  type: 'num',    manual: true  },
  { key: 'gold_duty_bcd',    label: 'Gold BCD',   width: 75,  type: 'num',    manual: false },
  { key: 'gold_duty_cons',   label: 'Gold(C)',    width: 70,  type: 'num',    manual: false },
  { key: 'silver_duty_cons', label: 'Silver(C)',  width: 75,  type: 'num',    manual: false },
  { key: 'sws_on_gold',      label: 'SWS Gold',   width: 70,  type: 'num',    manual: true  },
  { key: 'aidc_gold_silver', label: 'AIDC G/S',   width: 70,  type: 'num',    manual: false },
  { key: 'sws_on_silver',    label: 'SWS Silv',   width: 70,  type: 'num',    manual: true  },
  { key: 'aidc_on_liquor',   label: 'AIDC Lqr',   width: 70,  type: 'num',    manual: false },
  { key: 'redemption_fine',  label: 'Redm.Fine',  width: 75,  type: 'num',    manual: true  },
  { key: 'reexport_fine',    label: 'Re-Exp.',    width: 70,  type: 'num',    manual: true  },
  { key: 'personal_penalty', label: 'Penalty',    width: 70,  type: 'num',    manual: true  },
  { key: 'other_charges',    label: 'Other',      width: 65,  type: 'num',    manual: true  },
  { key: 'fuel_duty',        label: 'Fuel',       width: 60,  type: 'num',    manual: true  },
  { key: 'cess_on_cig',      label: 'Cess/Cig',   width: 60,  type: 'num',    manual: true  },
  { key: 'total_duty',       label: 'TOTAL',      width: 80,  type: 'num',    manual: false },
  { key: 'flight_no',        label: 'Flight',     width: 75,  type: 'text',   manual: true  },
  { key: 'is_sbi_challan',   label: 'SBI',        width: 40,  type: 'bool',   manual: true  },
] as const;

type EntryKey = (typeof ENTRY_COLS)[number]['key'];

function numFmt(v: number | null | undefined): string {
  if (!v) return '';
  return v.toLocaleString('en-IN');
}

function safeParseOverrides(raw: string | null | undefined): string[] {
  try { return JSON.parse(raw || '[]'); } catch { return []; }
}

/** Re-add the row, unless the officer has written the total in themselves. */
function retotal(entry: DrEntry) {
  if (safeParseOverrides(entry.overrides).includes('total_duty')) return;
  entry.total_duty = computeTotal(entry);
}

export default function RevenueSheet({ session: initialSession, rules, onMessage, onConfig, onRulesChanged }: Props) {
  const [session, setSession] = useState<DrSession>(initialSession);
  const [entries, setEntries] = useState<DrEntry[]>(
    initialSession.entries.length > 0
      ? initialSession.entries
      : [blankEntry(0)]
  );
  const [drEntries, setDrEntries] = useState<DrDrEntry[]>(
    initialSession.dr_entries.length > 0 ? initialSession.dr_entries : []
  );
  const [osEntries, setOsEntries] = useState<DrOsEntry[]>(
    initialSession.os_entries.length > 0 ? initialSession.os_entries : []
  );
  const [itemTypes, setItemTypes] = useState<ItemType[]>([]);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(true);
  const [showDr, setShowDr] = useState(drEntries.length > 0);
  const [showOs, setShowOs] = useState(osEntries.length > 0);
  const [submitConfirm, setSubmitConfirm] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const tariff = session.tariff;

  // Gold weight validation — live, runs on every entry change
  const validationErrors = useMemo(() => {
    const errs = new Set<string>();
    entries.forEach((entry, idx) => {
      const desc = entry.item_desc.toUpperCase().trim();
      if ((desc === 'GOLD' || desc === 'GOLD(C)') && !entry.gold_weight_gms) {
        errs.add(`${idx}-gold_weight_gms`);
      }
    });
    return errs;
  }, [entries]);

  const [showValidationBanner, setShowValidationBanner] = useState(false);

  // Auto-hide the banner once all errors are corrected
  useEffect(() => {
    if (validationErrors.size === 0) setShowValidationBanner(false);
  }, [validationErrors.size]);

  // Ref so doSave (which is memoised separately) can read current errors without extra deps
  const validationErrorsRef = useRef(validationErrors);
  useEffect(() => { validationErrorsRef.current = validationErrors; }, [validationErrors]);

  // Keyboard navigation: (rowIdx, colIdx) of focused cell
  const [focusCell, setFocusCell] = useState<[number, number] | null>(null);
  const cellRefs = useRef<Map<string, HTMLInputElement | HTMLElement>>(new Map());

  useEffect(() => {
    api.get('/dcr/item-types')
      .then(r => setItemTypes(listOf<ItemType>(r.data)))
      .catch(() => {});
  }, []);

  // Auto-save debounce
  const saveTimer = useRef<number | undefined>(undefined);

  const doSave = useCallback(async (): Promise<boolean> => {
    setSaving(true);
    try {
      const res = await api.put(`/dcr/sessions/${initialSession.id}/entries`, {
        entries: entries.map((e, i) => ({ ...e, sort_order: i })),
        dr_entries: drEntries.map((e, i) => ({ ...e, sort_order: i })),
        os_entries: osEntries.map((e, i) => ({ ...e, sort_order: i })),
      });
      setSession(res.data);
      setSaved(true);
      return true;
    } catch {
      return false;
    } finally {
      setSaving(false);
    }
  }, [entries, drEntries, osEntries, initialSession.id]);

  // Ref so scheduleAutoSave (stable, empty deps) always calls the latest doSave.
  // Without this, the timeout fires the render-0 doSave with stale initial entries.
  const doSaveRef = useRef(doSave);
  useEffect(() => { doSaveRef.current = doSave; }, [doSave]);

  const scheduleAutoSave = useCallback(() => {
    setSaved(false);
    clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => doSaveRef.current(), 3000);
  }, []); // eslint-disable-line

  // Update an entry field with auto-compute on relevant fields
  const updateEntry = useCallback((idx: number, field: EntryKey, rawValue: unknown) => {
    setEntries(prev => {
      const next = [...prev];
      const entry = { ...next[idx] };

      if (field === 'is_sbi_challan') {
        entry.is_sbi_challan = rawValue as boolean;
      } else if (field === 'is_offline_br') {
        entry.is_offline_br = rawValue as boolean;
      } else if (field === 'item_desc') {
        // Capture auto-fields from the OLD item BEFORE changing item_desc.
        // Columns auto for the old item that have no matching rule for the new item
        // would otherwise silently retain stale values and inflate total_duty.
        const oldAutoFields = tariff ? getAutoFields(rules, entry.item_desc) : new Set<string>();

        entry.item_desc = rawValue as string;
        if (tariff) {
          const computed = computeDuties(tariff as DrTariff, rules, rawValue as string, entry.dutiable_value, entry.gold_weight_gms);
          // Zero duty columns that applied to the old item but have no rule for the new item
          for (const col of oldAutoFields) {
            if (!(col in computed)) (entry as Record<string, unknown>)[col] = 0;
          }
          Object.assign(entry, computed);
          retotal(entry);
          // clear overrides for auto-fields when item changes
          const autoFields = getAutoFields(rules, rawValue as string);
          const overrides = safeParseOverrides(entry.overrides);
          entry.overrides = JSON.stringify(overrides.filter(f => !autoFields.has(f)));
        }
      } else if (field === 'dutiable_value' || field === 'gold_weight_gms') {
        (entry as Record<string, unknown>)[field] = Number(rawValue) || 0;
        if (tariff) {
          const computed = computeDuties(tariff as DrTariff, rules, entry.item_desc, entry.dutiable_value, entry.gold_weight_gms);
          // Only overwrite non-overridden auto-computed fields
          const overrides = safeParseOverrides(entry.overrides);
          for (const [k, v] of Object.entries(computed)) {
            if (!overrides.includes(k)) (entry as Record<string, unknown>)[k] = v;
          }
          retotal(entry);
        }
      } else if (field === 'total_duty') {
        // The total is the sum of every money column, and an officer who can see
        // that it is wrong should be able to say so. A figure typed here stands:
        // the columns underneath are left exactly as they are, and no later edit
        // on the row recomputes it away — today or when the sheet is reopened.
        //
        // Emptying the cell hands it back to the formula. An override has to be
        // undoable, or one slip of the keyboard is permanent.
        const overrides = safeParseOverrides(entry.overrides);
        if (String(rawValue).trim() === '') {
          entry.overrides = JSON.stringify(overrides.filter(f => f !== 'total_duty'));
          entry.total_duty = computeTotal(entry);
        } else {
          entry.total_duty = Number(rawValue) || 0;
          if (!overrides.includes('total_duty')) {
            entry.overrides = JSON.stringify([...overrides, 'total_duty']);
          }
        }
      } else if (ENTRY_COLS.find(c => c.key === field)?.type === 'num') {
        const numVal = Number(rawValue) || 0;
        (entry as Record<string, unknown>)[field] = numVal;

        // Mark as overridden if it's an auto-field being manually set
        const autoFields = getAutoFields(rules, entry.item_desc);
        if (autoFields.has(field)) {
          const overrides = safeParseOverrides(entry.overrides);
          if (!overrides.includes(field)) overrides.push(field);
          entry.overrides = JSON.stringify(overrides);
        }
        retotal(entry);
      } else {
        (entry as Record<string, unknown>)[field] = rawValue;
      }

      next[idx] = entry;
      return next;
    });
    scheduleAutoSave();
  }, [tariff, rules, scheduleAutoSave]);

  const addRow = useCallback((afterIdx?: number) => {
    setEntries(prev => {
      const next = [...prev];
      const newEntry = blankEntry(next.length);
      if (afterIdx !== undefined) next.splice(afterIdx + 1, 0, newEntry);
      else next.push(newEntry);
      return next;
    });
    scheduleAutoSave();
  }, [scheduleAutoSave]);

  /**
   * Another item on the same receipt.
   *
   * One BR often covers several things — a laptop, a bottle, a phone — and each
   * gets its own line so its duty is worked out on its own. Officers were doing
   * that by adding a row and typing the receipt number again, which is where a
   * digit gets dropped and the items end up on two different receipts.
   *
   * This puts the line in directly below, with the receipt number and the flight
   * already on it, and drops the cursor on the item. Nothing else is copied: the
   * value and the duty belong to the item, not to the receipt.
   */
  const addItemToBr = useCallback((idx: number) => {
    setEntries(prev => {
      const from = prev[idx];
      if (!from) return prev;
      const next = [...prev];
      next.splice(idx + 1, 0, {
        ...blankEntry(next.length),
        br_no: from.br_no,
        flight_no: from.flight_no,
        is_offline_br: from.is_offline_br,
        is_sbi_challan: from.is_sbi_challan,
      });
      return next;
    });
    scheduleAutoSave();
    // onto the item description of the line just made
    const itemCol = ENTRY_COLS.findIndex(c => c.key === 'item_desc');
    setTimeout(() => cellRefs.current.get(`${idx + 1}-${itemCol}`)?.focus(), 0);
  }, [scheduleAutoSave]);

  /**
   * The right-click menu on a row of the sheet.
   *
   * The row's own actions sit behind a hover, which is fine once you know they
   * are there. A right-click is where anyone looks first, and it can say which
   * receipt it is about — "Add item to BR 49773" rather than an unlabelled plus,
   * so nobody has to work out which row the cursor was on.
   */
  const [rowMenu, setRowMenu] = useState<{ x: number; y: number; row: number; col: number | null } | null>(null);
  /** The column whose formula is being rewritten, and the officer's signature. */
  const [formulaEdit, setFormulaEdit] = useState<
    { rule: DrFormulaRule; expression: string; username: string; password: string;
      busy: boolean; error: string | null } | null>(null);

  useEffect(() => {
    if (!rowMenu) return;
    const close = () => setRowMenu(null);
    const onKey = (ev: KeyboardEvent) => { if (ev.key === 'Escape') close(); };
    // capture: a click anywhere dismisses it, including inside the sheet
    document.addEventListener('mousedown', close);
    document.addEventListener('keydown', onKey);
    window.addEventListener('resize', close);
    return () => {
      document.removeEventListener('mousedown', close);
      document.removeEventListener('keydown', onKey);
      window.removeEventListener('resize', close);
    };
  }, [rowMenu]);

  /**
   * The same thing again, on the next receipt.
   *
   * A shift is full of near-identical lines — four passengers off the same
   * flight with the same phone, at the same value. Everything is carried over
   * except what must differ: the receipt number and any O.S. it was linked to.
   */
  const duplicateRow = useCallback((idx: number) => {
    setEntries(prev => {
      const from = prev[idx];
      if (!from) return prev;
      const next = [...prev];
      next.splice(idx + 1, 0, { ...from, id: undefined, br_no: '', os_ref: '' });
      return next;
    });
    scheduleAutoSave();
    const brCol = ENTRY_COLS.findIndex(c => c.key === 'br_no');
    setTimeout(() => cellRefs.current.get(`${idx + 1}-${brCol}`)?.focus(), 0);
  }, [scheduleAutoSave]);

  const deleteRow = useCallback((idx: number) => {
    setEntries(prev => prev.length <= 1 ? [blankEntry(0)] : prev.filter((_, i) => i !== idx));
    scheduleAutoSave();
  }, [scheduleAutoSave]);

  const addItemType = useCallback(async (name: string) => {
    const res = await api.post('/dcr/item-types', { name });
    setItemTypes(prev => [...prev, res.data]);
  }, []);

  /**
   * An officer typed an O.S. number. The register already records which receipts
   * belong to that case, so fill the receipt column from it rather than ask them
   * to copy it across during a shift change.
   *
   * Deliberately timid: it fills the field only when empty, never overwrites what
   * someone typed, and stays silent when the case is unknown or the lookup fails.
   * A blank column is a normal outcome and must not interrupt the sheet.
   */
  /**
   * Receipts whose items are not all together.
   *
   * A shift is busy and a receipt sometimes gets typed twice — once here, once
   * fifteen rows down — either because the officer lost their place or because
   * a second item was added later. Both readings are trouble: if it is the same
   * item twice the shift is over-counted, and if it is a second item the two
   * will not club, so the receipt is written as though it were two.
   *
   * The arithmetic cannot tell which it is, and neither can we. It is marked so
   * the officer can look, and nothing is changed for them.
   */
  const splitReceipts = useMemo(() => {
    const runs = new Map<string, number>();
    let prev = '';
    for (const e of entries) {
      const br = (e.br_no || '').trim();
      if (br && br !== prev) runs.set(br, (runs.get(br) ?? 0) + 1);
      prev = br;
    }
    return new Set([...runs].filter(([, n]) => n > 1).map(([br]) => br));
  }, [entries]);

  /**
   * Where the case under the cursor stands.
   *
   * The sheet is where an officer sees the money arrive, so it is where they
   * should be able to see whether the case it belongs to is settled. The server
   * decides that across every receipt that paid the case; this only shows it.
   */
  const focusedCase = useMemo(() => {
    if (!focusCell) return null;
    const e = entries[focusCell[0]];
    const ref = (e?.os_ref || '').trim();
    if (!ref || !e?.status) return null;
    return { ref, status: e.status };
  }, [focusCell, entries]);

  /**
   * What the receipt under the cursor comes to, all its items together.
   *
   * One BR is one receipt for one amount, but its items are written a row each,
   * so the figure the officer has to check against the paper in their hand is
   * never on the screen — they add the rows up themselves. This puts it in the
   * status bar, the way a spreadsheet shows the sum of what you have selected.
   * It appears only for a receipt that actually spans rows; on an ordinary
   * one-item line the row total is already the receipt total.
   */
  const focusedReceipt = useMemo(() => {
    if (!focusCell) return null;
    const br = (entries[focusCell[0]]?.br_no || '').trim();
    if (!br) return null;
    const group = entries.filter(e => (e.br_no || '').trim() === br);
    if (group.length < 2) return null;
    return { br, count: group.length, total: group.reduce((t, e) => t + (e.total_duty || 0), 0) };
  }, [focusCell, entries]);

  // What the hand-set totals come to, against what the columns say. The report's
  // consolidated sheet adds the columns up head by head, so any difference here
  // is one the officer will see between the two sheets. It is shown rather than
  // quietly corrected: the figure they typed is the one they meant.
  const handTotalled = useMemo(() => {
    let count = 0, delta = 0;
    for (const e of entries) {
      if (!safeParseOverrides(e.overrides).includes('total_duty')) continue;
      count += 1;
      delta += (e.total_duty || 0) - computeTotal(e);
    }
    return { count, delta };
  }, [entries]);

  const handleOsRefBlur = useCallback((idx: number) => {
    const entry = entries[idx];
    const ref = (entry?.os_ref || '').trim();
    if (!ref) return;
    if ((entry.br_no || '').trim()) return;   // never touch what was typed
    api.get('/dcr/receipts-for-os', { params: { os_ref: ref } })
      .then(res => {
        const brs: string[] = res.data?.br_numbers ?? [];
        if (brs.length > 0) updateEntry(idx, 'br_no', brs.join(', '));
      })
      .catch(() => { /* unknown case, or offline — the officer types it as before */ });
  }, [entries, updateEntry]);

  const handleItemBlur = useCallback((idx: number) => {
    // Increment usage count in background
    const entry = entries[idx];
    if (!entry.item_desc) return;
    const found = itemTypes.find(it => it.name.toUpperCase() === entry.item_desc.toUpperCase());
    if (found) {
      api.patch(`/dcr/item-types/${found.id}/use`).catch(() => {});
      setItemTypes(prev => prev.map(it =>
        it.id === found.id ? { ...it, usage_count: it.usage_count + 1 } : it
      ));
    }
  }, [entries, itemTypes]);

  // Keyboard navigation between cells
  const handleCellKeyDown = useCallback((
    e: React.KeyboardEvent,
    rowIdx: number,
    colIdx: number,
  ) => {
    if (e.key === 'Tab') {
      e.preventDefault();
      const nextCol = e.shiftKey ? colIdx - 1 : colIdx + 1;
      if (nextCol >= 0 && nextCol < ENTRY_COLS.length) {
        const ref = cellRefs.current.get(`${rowIdx}-${nextCol}`);
        (ref as HTMLElement | undefined)?.focus();
      } else if (!e.shiftKey && nextCol >= ENTRY_COLS.length) {
        // Tab past last col: add row and go to first editable col of next row
        if (rowIdx === entries.length - 1) addRow();
        setTimeout(() => {
          const ref = cellRefs.current.get(`${rowIdx + 1}-1`);
          (ref as HTMLElement | undefined)?.focus();
        }, 50);
      }
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      if (rowIdx === entries.length - 1) addRow();
      const ref = cellRefs.current.get(`${rowIdx + 1}-${colIdx}`);
      (ref as HTMLElement | undefined)?.focus();
    }
    if (e.key === 'ArrowDown' && !e.shiftKey) {
      const ref = cellRefs.current.get(`${rowIdx + 1}-${colIdx}`);
      if (ref) { e.preventDefault(); (ref as HTMLElement).focus(); }
    }
    if (e.key === 'ArrowUp' && !e.shiftKey) {
      const ref = cellRefs.current.get(`${rowIdx - 1}-${colIdx}`);
      if (ref) { e.preventDefault(); (ref as HTMLElement).focus(); }
    }
  }, [entries.length, addRow]);

  // Column totals
  const totals = useMemo(() => {
    const t: Record<string, number> = {};
    for (const col of ENTRY_COLS) {
      if (col.type === 'num') {
        t[col.key] = entries.reduce((s, e) => s + ((e[col.key as keyof DrEntry] as number) || 0), 0);
      }
    }
    return t;
  }, [entries]);

  // Manual save: show validation banner if any errors, then save
  const handleSave = useCallback(async () => {
    if (validationErrorsRef.current.size > 0) setShowValidationBanner(true);
    await doSave();
  }, [doSave]);

  const doSubmit = useCallback(async () => {
    setSubmitting(true);
    try {
      const ok = await doSave();
      if (!ok) return;
      const res = await api.post(`/dcr/sessions/${initialSession.id}/submit`, {
        submitted_by: 'Officer',
      });
      setSession(res.data);
    } catch { /* silent */ }
    finally {
      setSubmitting(false);
      setSubmitConfirm(false);
    }
  }, [doSave, initialSession.id]);

  const doUnsubmit = useCallback(async () => {
    try {
      const res = await api.post(`/dcr/sessions/${initialSession.id}/unsubmit`);
      setSession(res.data);
    } catch { /* silent */ }
  }, [initialSession.id]);

  const downloadAdc = async () => {
    if (validationErrorsRef.current.size > 0) setShowValidationBanner(true);
    const ok = await doSave();
    if (!ok) return;
    // Generate ADC Excel client-side using current session entries
    downloadAdcExcel(session, entries);
  };

  const downloadRevenue = async () => {
    if (validationErrorsRef.current.size > 0) setShowValidationBanner(true);
    const ok = await doSave();
    if (!ok) return;
    try {
      // Fetch all sessions for the same date to get both shifts
      const listRes = await api.get('/dcr/sessions', { params: { date: session.report_date } });
      const sessionsForDate = listOf<DrSession>(listRes.data);

      const otherShift = session.shift === 'DAY' ? 'NIGHT' : 'DAY';
      const otherMeta = sessionsForDate.find(s => s.shift === otherShift);

      // Fetch the other shift's full data (with entries) if it exists
      let otherFull: DrSession | null = null;
      if (otherMeta) {
        const res = await api.get(`/dcr/sessions/${otherMeta.id}`);
        otherFull = res.data;
      }

      const daySession   = session.shift === 'DAY'   ? session   : otherFull;
      const nightSession = session.shift === 'NIGHT' ? session   : otherFull;

      downloadRevenueExcel(session.report_date, daySession, nightSession);
    } catch { /* silent */ }
  };

  const cellClass = (entry: DrEntry, field: string): string => {
    const autoFields = getAutoFields(rules, entry.item_desc);
    const overrides = safeParseOverrides(entry.overrides);
    const isAuto = autoFields.has(field);
    const isOverridden = overrides.includes(field);

    if (isOverridden) return 'bg-amber-50';
    if (isAuto) return 'bg-sky-50';
    return 'bg-white';
  };

  return (
    <div className="flex flex-col h-full bg-slate-50">
      {/* Header bar */}
      <div className="flex items-center gap-3 px-4 py-2 bg-white border-b border-slate-200 shrink-0">
        <div className="flex-1 min-w-0">
          <h2 className="text-sm font-bold text-slate-800 truncate">
            {session.shift} SHIFT · {fmtDate(session.report_date)} · {session.batch_name}
          </h2>
          <p className="text-xs text-slate-500">
            {tariff ? `Tariff: ${tariff.label ?? fmtDate(tariff.effective_from)} (Baggage ${(tariff.baggage_rate * 100).toFixed(0)}%)` : 'No tariff'}
          </p>
        </div>

        <div className="flex items-center gap-2 shrink-0">
          {/* Save status */}
          <span className={`text-xs ${saving ? 'text-amber-500' : saved ? 'text-emerald-600' : 'text-slate-400'}`}>
            {saving ? 'Saving…' : saved ? '✓ Saved' : '● Unsaved'}
          </span>

          <button onClick={handleSave} disabled={saving}
            className={`flex items-center gap-1.5 px-3 py-1.5 text-xs font-semibold text-white rounded-lg disabled:opacity-60
              ${validationErrors.size > 0 ? 'bg-amber-600 hover:bg-amber-700' : 'bg-teal-600 hover:bg-teal-700'}`}
            title={validationErrors.size > 0 ? `${validationErrors.size} field(s) need attention` : 'Save'}
          >
            <Save size={13} />
            Save{validationErrors.size > 0 ? ` (${validationErrors.size}!)` : ''}
          </button>

          <button onClick={onConfig}
            className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-semibold bg-slate-100 hover:bg-slate-200 text-slate-700 rounded-lg">
            <Settings2 size={13} /> Config
          </button>

          <button onClick={onMessage}
            className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-semibold bg-slate-100 hover:bg-slate-200 text-slate-700 rounded-lg">
            <MessageSquare size={13} /> Message
          </button>

          <button onClick={downloadAdc}
            className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-semibold bg-blue-600 hover:bg-blue-700 text-white rounded-lg">
            <Download size={13} /> ADC
          </button>

          <button onClick={downloadRevenue}
            className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-semibold bg-violet-600 hover:bg-violet-700 text-white rounded-lg">
            <Download size={13} /> Revenue
          </button>
        </div>
      </div>

      {/* Legend */}
      <div className="flex items-center gap-4 px-4 py-1 bg-slate-50 border-b border-slate-200 text-[10px] text-slate-500 shrink-0">
        <span className="flex items-center gap-1"><span className="w-3 h-3 rounded bg-sky-100 border border-sky-200 inline-block" /> Auto-computed</span>
        <span className="flex items-center gap-1"><span className="w-3 h-3 rounded bg-amber-100 border border-amber-200 inline-block" /> Overridden</span>
        <span className="flex items-center gap-1"><span className="w-3 h-3 rounded bg-red-100 border border-red-200 inline-block" /> SBI Challan</span>
        <span className="flex items-center gap-1 text-red-600 font-semibold"><AlertTriangle size={10} /> Gold(g)* — weight mandatory for GOLD / GOLD(C) items</span>
        <span className="flex items-center gap-1"><span className="text-red-600 font-bold text-[10px]">BR</span> Offl. ☑ = Offline (manual) BR — shown in red</span>
      </div>

      {/* Validation banner — shown when Save/Download is clicked with pending errors */}
      {showValidationBanner && validationErrors.size > 0 && (
        <div className="px-4 py-2 bg-red-50 border-b-2 border-red-400 shrink-0 animate-pulse-once">
          <div className="flex items-start gap-2">
            <AlertTriangle size={15} className="text-red-500 shrink-0 mt-0.5" />
            <div className="flex-1 min-w-0">
              <p className="text-xs font-bold text-red-700">
                {validationErrors.size} mandatory field{validationErrors.size > 1 ? 's are' : ' is'} missing — please fix before downloading:
              </p>
              <ul className="mt-1 space-y-0.5 list-disc list-inside">
                {entries.map((entry, idx) => {
                  const desc = entry.item_desc.toUpperCase().trim();
                  if ((desc === 'GOLD' || desc === 'GOLD(C)') && !entry.gold_weight_gms) {
                    return (
                      <li key={idx} className="text-[11px] text-red-600">
                        Row {idx + 1} &mdash; <strong>{entry.item_desc}</strong>: Gold weight (grams) is required and must be &gt; 0
                      </li>
                    );
                  }
                  return null;
                })}
              </ul>
            </div>
            <button onClick={() => setShowValidationBanner(false)} className="text-red-400 hover:text-red-600 shrink-0 p-0.5" title="Dismiss">
              <X size={14} />
            </button>
          </div>
        </div>
      )}

      {/* Main scrollable area */}
      <div className="flex-1 overflow-auto">
        {/* Entry table */}
        <table className="text-xs border-collapse" style={{ minWidth: '1800px' }}>
          <thead className="sticky top-0 z-10">
            <tr className="bg-slate-800 text-white">
              <th className="w-8 px-1" />
              {ENTRY_COLS.map(col => (
                <th
                  key={col.key}
                  style={{ width: col.width, minWidth: col.width }}
                  className="px-1 py-1.5 text-center font-semibold text-[10px] uppercase tracking-wide border-r border-slate-700 whitespace-nowrap"
                  title={col.key === 'gold_weight_gms' ? 'Required for GOLD and GOLD(C) items' : undefined}
                >
                  {col.label}{col.key === 'gold_weight_gms' ? <span className="text-red-400 ml-0.5">*</span> : null}
                </th>
              ))}
              <th className="w-14 px-1" />
            </tr>
          </thead>

          <tbody>
            {entries.map((entry, rowIdx) => {
              const isSbi = entry.is_sbi_challan;
              const rowBg = isSbi ? 'bg-red-50' : rowIdx % 2 === 0 ? 'bg-white' : 'bg-slate-50';

              return (
                <tr key={rowIdx} className={`${rowBg} hover:bg-blue-50/50 group border-b border-slate-200`}
                  onContextMenu={ev => {
                    ev.preventDefault();
                    const cell = (ev.target as HTMLElement).closest('td');
                    const col = cell ? Number(cell.dataset.colIdx ?? '') : NaN;
                    setRowMenu({ x: ev.clientX, y: ev.clientY, row: rowIdx,
                                 col: Number.isFinite(col) ? col : null });
                  }}
                >
                  {/* Row number */}
                  <td className="text-center text-[10px] text-slate-400 px-1 border-r border-slate-200 w-8">
                    {rowIdx + 1}
                  </td>

                  {ENTRY_COLS.map((col, colIdx) => {
                    const fieldKey = col.key as EntryKey;
                    const val = entry[fieldKey as keyof DrEntry];
                    const bg = isSbi ? '' : cellClass(entry, fieldKey);

                    if (col.type === 'bool') {
                      const accentClass = col.key === 'is_offline_br' ? 'accent-orange-500' : 'accent-red-500';
                      return (
                        <td key={col.key} data-col-idx={colIdx} style={{ width: col.width }}
                          className={`${bg} border-r border-slate-200 text-center`}
                          title={col.key === 'is_offline_br' ? (val ? 'Offline BR (manual)' : 'Online BR') : undefined}
                        >
                          <input
                            type="checkbox"
                            checked={!!val}
                            onChange={e => updateEntry(rowIdx, fieldKey, e.target.checked)}
                            className={`w-3.5 h-3.5 ${accentClass}`}
                            ref={el => {
                              if (el) cellRefs.current.set(`${rowIdx}-${colIdx}`, el);
                            }}
                          />
                        </td>
                      );
                    }

                    if (col.type === 'combo') {
                      return (
                        <td key={col.key} data-col-idx={colIdx} style={{ width: col.width }} className={`${bg} border-r border-slate-200 p-0`}>
                          <ItemCombobox
                            value={entry.item_desc}
                            items={itemTypes}
                            onChange={v => updateEntry(rowIdx, 'item_desc', v)}
                            onBlur={() => handleItemBlur(rowIdx)}
                            onAddNew={addItemType}
                            className="w-full"
                          />
                        </td>
                      );
                    }

                    if (col.type === 'int' && fieldKey === 'sl_no') {
                      return (
                        <td key={col.key} data-col-idx={colIdx} style={{ width: col.width }} className={`${bg} border-r border-slate-200 px-1 text-center`}>
                          <input
                            type="number"
                            value={(val as number) ?? ''}
                            placeholder="–"
                            onChange={e => updateEntry(rowIdx, fieldKey, e.target.value === '' ? null : Number(e.target.value))}
                            onKeyDown={e => handleCellKeyDown(e, rowIdx, colIdx)}
                            onFocus={() => setFocusCell([rowIdx, colIdx])}
                            className="w-full text-center bg-transparent focus:outline-none focus:bg-blue-50 rounded"
                            ref={el => { if (el) cellRefs.current.set(`${rowIdx}-${colIdx}`, el); }}
                          />
                        </td>
                      );
                    }

                    if (col.type === 'num') {
                      const autoFields = getAutoFields(rules, entry.item_desc);
                      const isAutoField = autoFields.has(fieldKey);
                      const overrides = safeParseOverrides(entry.overrides);
                      const isOverridden = overrides.includes(fieldKey);
                      const displayVal = (val as number) ? numFmt(val as number) : '';
                      const hasError = validationErrors.has(`${rowIdx}-${fieldKey}`);

                      return (
                        <td key={col.key} data-col-idx={colIdx} style={{ width: col.width }}
                          className={`${hasError ? 'bg-red-100' : (isSbi ? '' : bg)} border-r ${hasError ? 'border-red-400 border-2' : 'border-slate-200'} p-0 relative`}>
                          {isOverridden && !hasError && (
                            <span className="absolute top-0 right-0 w-2 h-2 bg-amber-400 rounded-bl" title="Manually overridden" />
                          )}
                          {hasError && (
                            <span className="absolute top-0 right-0 w-2 h-2 bg-red-500 rounded-bl" title="Required field missing" />
                          )}
                          <input
                            type="number"
                            value={(val as number) || ''}
                            placeholder={hasError ? '⚠ Required' : (isAutoField && !isOverridden ? displayVal || '0' : '')}
                            onChange={e => updateEntry(rowIdx, fieldKey, e.target.value)}
                            onKeyDown={e => handleCellKeyDown(e, rowIdx, colIdx)}
                            onFocus={() => setFocusCell([rowIdx, colIdx])}

                            className={`w-full px-1 py-1 text-right focus:outline-none focus:bg-blue-100 rounded text-xs
                              ${col.key === 'total_duty' ? 'font-bold' : ''}
                              ${hasError ? 'bg-red-100 text-red-700 placeholder-red-400 font-semibold' : 'bg-transparent ' + (isAutoField && !isOverridden ? 'text-blue-700' : 'text-slate-800')}
                            `}
                            ref={el => { if (el) cellRefs.current.set(`${rowIdx}-${colIdx}`, el); }}
                          />
                        </td>
                      );
                    }

                    // text — BR No. turns red+bold when is_offline_br is true
                    const isBrNoCell = col.key === 'br_no';
                    const isOffline = entry.is_offline_br;
                    const isSplit = isBrNoCell && splitReceipts.has((entry.br_no || '').trim());
                    return (
                      <td key={col.key} data-col-idx={colIdx} style={{ width: col.width }}
                        title={isSplit
                          ? 'This receipt number also appears elsewhere in the sheet. '
                            + 'If it is the same item twice the shift is counted twice; '
                            + 'if it is another item, move it next to the first so they stay together.'
                          : undefined}
                        className={`${bg} border-r border-slate-200 p-0 ${isSplit ? 'ring-1 ring-inset ring-amber-400 bg-amber-50' : ''}`}>
                        <input
                          type="text"
                          value={(val as string) || ''}
                          onChange={e => updateEntry(rowIdx, fieldKey, e.target.value)}
                          onKeyDown={e => handleCellKeyDown(e, rowIdx, colIdx)}
                          onFocus={() => setFocusCell([rowIdx, colIdx])}
                          onBlur={fieldKey === 'os_ref' ? () => handleOsRefBlur(rowIdx) : undefined}
                          className={`w-full px-1 py-1 text-xs bg-transparent focus:outline-none focus:bg-blue-100 rounded
                            ${isBrNoCell && isOffline ? 'text-red-600 font-bold' : ''}
                          `}
                          ref={el => { if (el) cellRefs.current.set(`${rowIdx}-${colIdx}`, el); }}
                        />
                      </td>
                    );
                  })}

                  {/* Row actions */}
                  <td className="w-14 border-r border-slate-200 text-center whitespace-nowrap">
                    <button
                      onClick={() => addItemToBr(rowIdx)}
                      title="Add another item to this BR"
                      disabled={!entry.br_no}
                      className="opacity-0 group-hover:opacity-100 text-blue-500 hover:text-blue-700 disabled:text-slate-300 p-0.5 transition-opacity"
                    >
                      <Plus size={12} />
                    </button>
                    <button
                      onClick={() => deleteRow(rowIdx)}
                      title="Delete this row"
                      className="opacity-0 group-hover:opacity-100 text-red-400 hover:text-red-600 p-0.5 transition-opacity"
                    >
                      <Trash2 size={12} />
                    </button>
                  </td>
                </tr>
              );
            })}

            {/* Totals row */}
            <tr className="bg-amber-50 border-t-2 border-amber-300 font-bold">
              <td />
              {ENTRY_COLS.map(col => (
                <td
                  key={col.key}
                  className="px-1 py-1.5 text-right text-xs border-r border-slate-200"
                >
                  {col.type === 'num'
                    ? numFmt(totals[col.key]) || '—'
                    : col.key === 'item_desc' ? 'TOTAL' : ''}
                </td>
              ))}
              <td />
            </tr>
          </tbody>
        </table>

        {/* Add row button */}
        <div className="px-4 py-2 bg-white border-b border-slate-200">
          <button
            onClick={() => addRow()}
            className="flex items-center gap-2 text-xs text-teal-700 hover:text-teal-900 font-semibold"
          >
            <Plus size={14} /> Add Row
          </button>
        </div>

        {/* DR Sub-table */}
        <div className="px-4 py-3 bg-white border-b border-slate-200">
          <button
            className="flex items-center gap-2 text-xs font-bold text-slate-700 mb-2"
            onClick={() => setShowDr(s => !s)}
          >
            {showDr ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            DR (Detention Register) Sub-table
            <span className="text-slate-400 font-normal">({drEntries.length} entries)</span>
          </button>

          {showDr && (
            <table className="text-xs border-collapse w-full max-w-3xl">
              <thead>
                <tr className="bg-slate-100 text-slate-600">
                  {['DR No.', 'Amount', 'Item Description', 'Remarks', ''].map(h => (
                    <th key={h} className="px-2 py-1 text-left border border-slate-200">{h}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {drEntries.map((e, i) => (
                  <tr key={i} className="border-b border-slate-100">
                    <td className="border border-slate-200 p-0">
                      <input className="w-full px-2 py-1 text-xs bg-transparent focus:outline-none focus:bg-blue-50"
                        value={e.dr_no} onChange={ev => {
                          const next = [...drEntries]; next[i] = { ...e, dr_no: ev.target.value };
                          setDrEntries(next); scheduleAutoSave();
                        }} />
                    </td>
                    <td className="border border-slate-200 p-0">
                      <input type="number" className="w-full px-2 py-1 text-xs text-right bg-transparent focus:outline-none focus:bg-blue-50"
                        value={e.amount || ''} onChange={ev => {
                          const next = [...drEntries]; next[i] = { ...e, amount: Number(ev.target.value) || 0 };
                          setDrEntries(next); scheduleAutoSave();
                        }} />
                    </td>
                    <td className="border border-slate-200 p-0">
                      <input className="w-full px-2 py-1 text-xs bg-transparent focus:outline-none focus:bg-blue-50"
                        value={e.item_desc} onChange={ev => {
                          const next = [...drEntries]; next[i] = { ...e, item_desc: ev.target.value };
                          setDrEntries(next); scheduleAutoSave();
                        }} />
                    </td>
                    <td className="border border-slate-200 p-0">
                      <input className="w-full px-2 py-1 text-xs bg-transparent focus:outline-none focus:bg-blue-50"
                        value={e.remarks} onChange={ev => {
                          const next = [...drEntries]; next[i] = { ...e, remarks: ev.target.value };
                          setDrEntries(next); scheduleAutoSave();
                        }} />
                    </td>
                    <td className="border border-slate-200 px-1 text-center">
                      <button onClick={() => { setDrEntries(drEntries.filter((_, j) => j !== i)); scheduleAutoSave(); }}
                        className="text-red-400 hover:text-red-600"><Trash2 size={12} /></button>
                    </td>
                  </tr>
                ))}
                <tr className="bg-slate-50 font-bold text-xs">
                  <td className="px-2 py-1 text-slate-500">TOTAL</td>
                  <td className="px-2 py-1 text-right">{numFmt(drEntries.reduce((s, e) => s + e.amount, 0))}</td>
                  <td colSpan={3} />
                </tr>
              </tbody>
            </table>
          )}
          {showDr && (
            <button
              onClick={() => { setDrEntries([...drEntries, blankDrEntry(drEntries.length)]); scheduleAutoSave(); }}
              className="mt-2 flex items-center gap-1.5 text-xs text-teal-700 hover:text-teal-900 font-semibold"
            >
              <Plus size={12} /> Add DR Entry
            </button>
          )}
          {!showDr && (
            <button onClick={() => { setShowDr(true); setDrEntries([...drEntries, blankDrEntry(drEntries.length)]); }}
              className="text-xs text-teal-600 hover:underline">+ Add DR entry</button>
          )}
        </div>

        {/* OS Sub-table */}
        <div className="px-4 py-3 bg-white">
          <button
            className="flex items-center gap-2 text-xs font-bold text-slate-700 mb-2"
            onClick={() => setShowOs(s => !s)}
          >
            {showOs ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            OS (Offence Sheet) Sub-table
            <span className="text-slate-400 font-normal">({osEntries.length} entries)</span>
          </button>

          {showOs && (
            <table className="text-xs border-collapse w-full max-w-3xl">
              <thead>
                <tr className="bg-slate-100 text-slate-600">
                  {['OS No.', 'Amount', 'Item Description', 'Remarks', ''].map(h => (
                    <th key={h} className="px-2 py-1 text-left border border-slate-200">{h}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {osEntries.map((e, i) => (
                  <tr key={i} className="border-b border-slate-100">
                    <td className="border border-slate-200 p-0">
                      <input className="w-full px-2 py-1 text-xs bg-transparent focus:outline-none focus:bg-blue-50"
                        value={e.os_no} onChange={ev => {
                          const next = [...osEntries]; next[i] = { ...e, os_no: ev.target.value };
                          setOsEntries(next); scheduleAutoSave();
                        }} />
                    </td>
                    <td className="border border-slate-200 p-0">
                      <input type="number" className="w-full px-2 py-1 text-xs text-right bg-transparent focus:outline-none focus:bg-blue-50"
                        value={e.amount || ''} onChange={ev => {
                          const next = [...osEntries]; next[i] = { ...e, amount: Number(ev.target.value) || 0 };
                          setOsEntries(next); scheduleAutoSave();
                        }} />
                    </td>
                    <td className="border border-slate-200 p-0">
                      <input className="w-full px-2 py-1 text-xs bg-transparent focus:outline-none focus:bg-blue-50"
                        value={e.item_desc} onChange={ev => {
                          const next = [...osEntries]; next[i] = { ...e, item_desc: ev.target.value };
                          setOsEntries(next); scheduleAutoSave();
                        }} />
                    </td>
                    <td className="border border-slate-200 p-0">
                      <input className="w-full px-2 py-1 text-xs bg-transparent focus:outline-none focus:bg-blue-50"
                        value={e.remarks} onChange={ev => {
                          const next = [...osEntries]; next[i] = { ...e, remarks: ev.target.value };
                          setOsEntries(next); scheduleAutoSave();
                        }} />
                    </td>
                    <td className="border border-slate-200 px-1 text-center">
                      <button onClick={() => { setOsEntries(osEntries.filter((_, j) => j !== i)); scheduleAutoSave(); }}
                        className="text-red-400 hover:text-red-600"><Trash2 size={12} /></button>
                    </td>
                  </tr>
                ))}
                <tr className="bg-slate-50 font-bold text-xs">
                  <td className="px-2 py-1 text-slate-500">TOTAL</td>
                  <td className="px-2 py-1 text-right">{numFmt(osEntries.reduce((s, e) => s + e.amount, 0))}</td>
                  <td colSpan={3} />
                </tr>
              </tbody>
            </table>
          )}
          {showOs && (
            <button
              onClick={() => { setOsEntries([...osEntries, blankOsEntry(osEntries.length)]); scheduleAutoSave(); }}
              className="mt-2 flex items-center gap-1.5 text-xs text-teal-700 hover:text-teal-900 font-semibold"
            >
              <Plus size={12} /> Add OS Entry
            </button>
          )}
          {!showOs && (
            <button onClick={() => { setShowOs(true); setOsEntries([...osEntries, blankOsEntry(osEntries.length)]); }}
              className="text-xs text-teal-600 hover:underline">+ Add OS entry</button>
          )}
        </div>
      </div>

      {/* Submit button bar */}
      <div className="px-4 py-2 bg-white border-t border-slate-200 flex items-center gap-3 shrink-0">
        {session.submitted_at ? (
          <>
            <span className="text-xs font-semibold text-emerald-700 flex items-center gap-1.5">
              ✓ Submitted — {session.shift} Shift Report
              <span className="text-slate-400 font-normal">
                by {session.submitted_by} on {new Date(session.submitted_at).toLocaleString()}
              </span>
            </span>
            <button
              onClick={doUnsubmit}
              className="ml-auto text-xs text-slate-500 hover:text-slate-700 underline"
            >
              Re-open
            </button>
          </>
        ) : (
          <>
            <span className="text-xs text-slate-500">When all entries are complete:</span>
            <button
              onClick={() => setSubmitConfirm(true)}
              className="ml-auto flex items-center gap-2 px-4 py-1.5 text-xs font-bold bg-emerald-600 hover:bg-emerald-700 text-white rounded-lg"
            >
              Submit {session.shift} Batch Revenue Report
            </button>
          </>
        )}
      </div>

      {/* Rewriting one column's formula, signed by the officer doing it. */}
      {formulaEdit && (
        <div className="fixed inset-0 z-[9996] flex items-center justify-center bg-slate-900/60 p-4">
          <div className="w-full max-w-lg rounded-xl bg-white shadow-xl border border-slate-200 overflow-hidden">
            <div className="px-5 py-3 border-b border-slate-200 bg-slate-50">
              <h2 className="text-sm font-semibold text-slate-800">
                Formula for {formulaEdit.rule.column_label || formulaEdit.rule.target_column}
              </h2>
            </div>
            <div className="px-5 py-4 space-y-3">
              <p className="text-xs text-slate-600 leading-relaxed">
                The new formula applies to shifts <strong>from today onwards</strong>. Sheets
                already written keep the figures they were given, and a sheet from an
                earlier date reopened later still computes on the formula that was in
                force on its own date. Only this column changes.
              </p>
              <label className="block">
                <span className="text-[11px] uppercase tracking-wide text-slate-500">Formula</span>
                <input
                  value={formulaEdit.expression}
                  onChange={e => setFormulaEdit(f => f && { ...f, expression: e.target.value })}
                  spellCheck={false}
                  className="mt-1 w-full px-2 py-1.5 text-sm font-mono border border-slate-300 rounded-lg
                             focus:outline-none focus:ring-2 focus:ring-blue-400"
                />
              </label>
              <p className="text-[11px] text-slate-500">
                Available: <span className="font-mono">value</span>,{' '}
                <span className="font-mono">weight</span>, and the tariff rates.
              </p>
              <div className="grid grid-cols-2 gap-3 pt-1">
                <label className="block">
                  <span className="text-[11px] uppercase tracking-wide text-slate-500">Your username</span>
                  <input
                    value={formulaEdit.username} autoComplete="off"
                    onChange={e => setFormulaEdit(f => f && { ...f, username: e.target.value })}
                    className="mt-1 w-full px-2 py-1.5 text-sm border border-slate-300 rounded-lg
                               focus:outline-none focus:ring-2 focus:ring-blue-400"
                  />
                </label>
                <label className="block">
                  <span className="text-[11px] uppercase tracking-wide text-slate-500">Your password</span>
                  <input
                    type="password" value={formulaEdit.password} autoComplete="off"
                    onChange={e => setFormulaEdit(f => f && { ...f, password: e.target.value })}
                    className="mt-1 w-full px-2 py-1.5 text-sm border border-slate-300 rounded-lg
                               focus:outline-none focus:ring-2 focus:ring-blue-400"
                  />
                </label>
              </div>
              {formulaEdit.error && (
                <p className="text-xs font-medium text-red-600">{formulaEdit.error}</p>
              )}
            </div>
            <div className="flex justify-end gap-2 px-5 py-3 bg-slate-50 border-t border-slate-100">
              <button
                onClick={() => setFormulaEdit(null)}
                className="px-3 py-2 text-xs rounded-lg text-slate-600 hover:bg-slate-200 font-medium"
              >
                Cancel
              </button>
              <button
                disabled={formulaEdit.busy || !formulaEdit.username || !formulaEdit.password}
                onClick={async () => {
                  setFormulaEdit(f => f && { ...f, busy: true, error: null });
                  try {
                    await api.post(`/dcr/formula-rules/${formulaEdit.rule.id}/revise`, {
                      expression: formulaEdit.expression,
                      username: formulaEdit.username,
                      password: formulaEdit.password,
                    });
                    onRulesChanged?.();      // take it up without reopening the sheet
                    setFormulaEdit(null);
                  } catch (err: unknown) {
                    const detail = (err as { response?: { data?: { error?: string; detail?: string } } })
                      ?.response?.data?.error
                      || (err as { response?: { data?: { detail?: string } } })?.response?.data?.detail
                      || 'Could not update the formula.';
                    setFormulaEdit(f => f && { ...f, busy: false, error: detail });
                  }
                }}
                className="px-4 py-2 text-xs rounded-lg bg-blue-600 text-white hover:bg-blue-700
                           disabled:opacity-60 font-semibold"
              >
                {formulaEdit.busy ? 'Saving…' : 'Update the formula'}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* The row's right-click menu, named for the receipt it acts on. */}
      {rowMenu && (() => {
        const row = entries[rowMenu.row];
        if (!row) return null;
        const br = (row.br_no || '').trim();
        const item = (label: string, onPick: () => void, disabled = false) => (
          <button
            key={label}
            disabled={disabled}
            onMouseDown={ev => { ev.stopPropagation(); }}
            onClick={() => { onPick(); setRowMenu(null); }}
            className="w-full text-left px-3 py-1.5 text-xs text-slate-700 hover:bg-blue-50
                       disabled:text-slate-300 disabled:hover:bg-transparent whitespace-nowrap"
          >
            {label}
          </button>
        );
        return (
          <div
            style={{
              // kept inside the window when the click lands near an edge
              left: Math.min(rowMenu.x, window.innerWidth - 240),
              top: Math.min(rowMenu.y, window.innerHeight - 150),
            }}
            className="fixed z-[9990] min-w-[13rem] py-1 bg-white rounded-lg shadow-xl border border-slate-200"
          >
            <div className="px-3 py-1 text-[10px] uppercase tracking-wide text-slate-400 border-b border-slate-100">
              {br ? `BR ${br}` : 'This row'}
            </div>
            {item(br ? `Add item to BR ${br}` : 'Add item to this BR',
                  () => addItemToBr(rowMenu.row), !br)}
            {(() => {
              // Only where the cursor is on a column a formula actually fills.
              const key = rowMenu.col !== null ? ENTRY_COLS[rowMenu.col]?.key : null;
              const rule = key ? rules.find(r => r.target_column === key && r.is_active) : undefined;
              if (!rule) return null;
              return item(`Update the formula for ${rule.column_label || rule.target_column}`,
                () => setFormulaEdit({ rule, expression: rule.expression || '',
                                       username: '', password: '', busy: false, error: null }));
            })()}
            {item('Duplicate, without the receipt number', () => duplicateRow(rowMenu.row))}
            {item('Insert an empty row below', () => addRow(rowMenu.row))}
            {safeParseOverrides(row.overrides).includes('total_duty') &&
              item('Put the total back on the formula',
                   () => updateEntry(rowMenu.row, 'total_duty', ''))}
            {item('Delete this row', () => deleteRow(rowMenu.row))}
          </div>
        );
      })()}

      {/* Footer status */}
      <div className="px-4 py-1.5 bg-slate-800 text-slate-300 text-[11px] flex items-center gap-4 shrink-0">
        <span>{entries.filter(e => e.br_no || e.item_desc).length} entries</span>
        {handTotalled.count > 0 && (
          <span
            className="text-amber-300"
            title={'The consolidated sheet adds the duty, fine and penalty columns up head by head, '
                 + 'so it will not match a total that was set by hand. Empty the cell to put a row '
                 + 'back on the formula.'}
          >
            {handTotalled.count} total{handTotalled.count > 1 ? 's' : ''} set by hand
            {Math.round(handTotalled.delta) !== 0 && (
              <> — ₹{Math.abs(Math.round(handTotalled.delta)).toLocaleString('en-IN')}
                {handTotalled.delta > 0 ? ' above' : ' below'} the columns</>
            )}
          </span>
        )}
        <span>Total Duty: <strong className="text-white">₹{Math.round(totals['total_duty'] || 0).toLocaleString('en-IN')}</strong></span>
        <span>DR: <strong className="text-white">₹{Math.round(drEntries.reduce((s, e) => s + e.amount, 0)).toLocaleString('en-IN')}</strong></span>
        <span>OS: <strong className="text-white">₹{Math.round(osEntries.reduce((s, e) => s + e.amount, 0)).toLocaleString('en-IN')}</strong></span>
        {focusedReceipt && (
          <span className="text-emerald-300" title="Every item on this receipt, added up">
            BR {focusedReceipt.br} · {focusedReceipt.count} items · ₹
            {Math.round(focusedReceipt.total).toLocaleString('en-IN')}
          </span>
        )}
        {focusedCase && (
          <span
            className={focusedCase.status === 'CLOSED' ? 'text-emerald-300'
                     : focusedCase.status === 'OPEN'   ? 'text-amber-300'
                     : 'text-slate-400'}
            title={focusedCase.status === 'LEGACY'
              ? 'From before the office began keeping cases the new way — whether it was settled is not recorded, so it is not called open or closed.'
              : 'Across every receipt on this sheet that paid this case.'}
          >
            O.S. {focusedCase.ref} · {focusedCase.status === 'LEGACY' ? 'LEGACY O.S.' : focusedCase.status}
          </span>
        )}
        {focusCell && (
          <span className="ml-auto text-slate-500">
            R{focusCell[0] + 1} C{focusCell[1] + 1}
          </span>
        )}
      </div>

      {/* Submit confirmation modal */}
      {submitConfirm && (
        <div className="fixed inset-0 bg-black/50 z-50 flex items-center justify-center p-4">
          <div className="bg-white rounded-2xl shadow-2xl max-w-md w-full p-6">
            <h3 className="text-base font-bold text-slate-800 mb-2">
              Submit {session.shift} Shift Revenue Report?
            </h3>
            <p className="text-sm text-slate-600 mb-1">
              <strong>{fmtDate(session.report_date)} · {session.shift} Shift · {session.batch_name}</strong>
            </p>
            <p className="text-sm text-slate-500 mt-3 mb-5">
              This will mark the report as submitted. The data is saved as-is.
              You can still make edits and re-submit if needed.
            </p>
            <div className="flex gap-3 justify-end">
              <button
                onClick={() => setSubmitConfirm(false)}
                className="px-4 py-2 text-sm font-medium text-slate-700 bg-slate-100 hover:bg-slate-200 rounded-lg"
              >
                Cancel
              </button>
              <button
                onClick={doSubmit}
                disabled={submitting}
                className="px-4 py-2 text-sm font-bold text-white bg-emerald-600 hover:bg-emerald-700 rounded-lg disabled:opacity-60"
              >
                {submitting ? 'Submitting…' : 'Confirm Submit'}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
