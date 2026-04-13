import { useState, useRef, useEffect, memo } from 'react';
import { ChevronLeft, ChevronRight, Calendar } from 'lucide-react';
import { getDaysInMonth, getDay, startOfMonth, addMonths, subMonths } from 'date-fns';

interface DatePickerProps {
  value: string;           // ISO yyyy-mm-dd, 'N.A.', or empty string
  onChange: (val: string) => void;
  id?: string;
  inputClassName: string;  // full className for the text input (no error classes needed)
  error?: boolean;
  placeholder?: string;
  allowNA?: boolean;       // allow 'N.A.' as a valid value
  minDate?: string;        // ISO yyyy-mm-dd — dates before this are disabled
}

const DAYS = ['Su', 'Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa'];
const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June',
  'July', 'August', 'September', 'October', 'November', 'December'];

function isoToDisplay(iso: string): string {
  if (!iso) return '';
  if (!/^\d{4}-\d{2}-\d{2}$/.test(iso)) return iso; // pass N.A. etc. through as-is
  const [y, m, d] = iso.split('-');
  return `${d}/${m}/${y}`;
}

function displayToIso(str: string): string | null {
  if (/^\d{2}\/\d{2}\/\d{4}$/.test(str)) {
    const [d, m, y] = str.split('/');
    const date = new Date(`${y}-${m}-${d}T00:00:00`);
    if (!isNaN(date.getTime())) {
      // Prevent Javascript from silently overflowing (e.g. 31/02/2026 -> 03/03/2026)
      // by ensuring the generated Date parts strictly match the user's input.
      if (
        String(date.getDate()).padStart(2, '0') === d &&
        String(date.getMonth() + 1).padStart(2, '0') === m &&
        String(date.getFullYear()) === y
      ) {
        return `${y}-${m}-${d}`;
      }
    }
  }
  return null;
}

const DatePicker = memo(function DatePicker({
  value, onChange, id, inputClassName, error, placeholder = 'dd/mm/yyyy', allowNA, minDate
}: DatePickerProps) {
  const [open, setOpen] = useState(false);
  const [typed, setTyped] = useState(() => isoToDisplay(value));
  const [viewDate, setViewDate] = useState<Date>(() =>
    value && /^\d{4}-\d{2}-\d{2}$/.test(value) ? new Date(value + 'T00:00:00') : new Date()
  );
  const [popupPos, setPopupPos] = useState({ top: 0, left: 0 });
  const btnRef = useRef<HTMLButtonElement>(null);
  const popupRef = useRef<HTMLDivElement>(null);

  // Keep display in sync when value changes externally (e.g. scanner fills form)
  useEffect(() => {
    setTyped(isoToDisplay(value));
    if (value && /^\d{4}-\d{2}-\d{2}$/.test(value)) {
      setViewDate(new Date(value + 'T00:00:00'));
    }
  }, [value]);

  // Close on outside click
  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (
        popupRef.current && !popupRef.current.contains(e.target as Node) &&
        btnRef.current && !btnRef.current.contains(e.target as Node)
      ) setOpen(false);
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [open]);

  // Close on Escape
  useEffect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent) => { if (e.key === 'Escape') setOpen(false); };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, [open]);

  const openCalendar = () => {
    if (btnRef.current) {
      const rect = btnRef.current.getBoundingClientRect();
      const popupW = 260;
      const left = Math.max(4, Math.min(rect.right - popupW, window.innerWidth - popupW - 4));
      // Flip above if too close to bottom
      const top = rect.bottom + 4 + 300 > window.innerHeight
        ? rect.top - 4 - 300
        : rect.bottom + 4;
      setPopupPos({ top, left });
    }
    if (value && /^\d{4}-\d{2}-\d{2}$/.test(value)) {
      setViewDate(new Date(value + 'T00:00:00'));
    }
    setOpen(v => !v);
  };

  const handleTextChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const raw = e.target.value;
    const val = allowNA ? raw.toUpperCase() : raw;
    setTyped(val);
    if (!val) { onChange(''); return; }
    if (allowNA && (val === 'N.A.' || val === 'NA')) { onChange(val); return; }
    const iso = displayToIso(val);
    if (iso) {
      onChange(iso);
      setViewDate(new Date(iso + 'T00:00:00'));
    }
  };

  const handleDayClick = (day: number) => {
    const y = viewDate.getFullYear();
    const m = String(viewDate.getMonth() + 1).padStart(2, '0');
    const d = String(day).padStart(2, '0');
    onChange(`${y}-${m}-${d}`);
    setOpen(false);
  };

  const year = viewDate.getFullYear();
  const month = viewDate.getMonth();
  const daysInMonth = getDaysInMonth(viewDate);
  const firstDayOfWeek = getDay(startOfMonth(viewDate));

  let selDay: number | null = null;
  let selYear: number | null = null;
  let selMonth: number | null = null;
  if (value && /^\d{4}-\d{2}-\d{2}$/.test(value)) {
    const [sy, sm, sd] = value.split('-').map(Number);
    selYear = sy; selMonth = sm - 1; selDay = sd;
  }

  const today = new Date();

  // minDate support
  const minDateObj = minDate && /^\d{4}-\d{2}-\d{2}$/.test(minDate)
    ? new Date(minDate + 'T00:00:00') : null;
  const isPastDay = (d: number) => {
    if (!minDateObj) return false;
    const candidate = new Date(year, month, d);
    candidate.setHours(0, 0, 0, 0);
    const min = new Date(minDateObj); min.setHours(0, 0, 0, 0);
    return candidate < min;
  };
  const isPrevMonthDisabled = minDateObj
    ? (viewDate.getFullYear() < minDateObj.getFullYear() ||
       (viewDate.getFullYear() === minDateObj.getFullYear() && viewDate.getMonth() <= minDateObj.getMonth()))
    : false;

  const errClass = error ? ' border-red-500 ring-1 ring-red-500' : '';

  return (
    <>
      <div className="flex items-center gap-1">
        <input
          id={id}
          type="text"
          value={typed}
          onChange={handleTextChange}
          placeholder={placeholder}
          className={inputClassName + errClass}
        />
        <button
          ref={btnRef}
          type="button"
          onClick={openCalendar}
          className="flex items-center justify-center w-8 h-8 rounded-md border border-slate-300 bg-white text-slate-500 hover:text-brand-600 hover:border-brand-400 transition-colors shrink-0"
          tabIndex={-1}
          title="Pick date"
        >
          <Calendar size={15} />
        </button>
      </div>

      {open && (
        <div
          ref={popupRef}
          className="fixed z-[9999] bg-white border border-slate-200 rounded-xl shadow-2xl p-3 select-none"
          style={{ top: popupPos.top, left: popupPos.left, width: 260 }}
        >
          {/* Month / year navigation */}
          <div className="flex items-center justify-between mb-2">
            <button
              type="button"
              onClick={() => { if (!isPrevMonthDisabled) setViewDate(subMonths(viewDate, 1)); }}
              disabled={isPrevMonthDisabled}
              className={`p-1.5 rounded text-slate-500 ${isPrevMonthDisabled ? 'opacity-30 cursor-not-allowed' : 'hover:bg-slate-100'}`}
            >
              <ChevronLeft size={15} />
            </button>
            <span className="text-sm font-semibold text-slate-700">
              {MONTHS[month]} {year}
            </span>
            <button
              type="button"
              onClick={() => setViewDate(addMonths(viewDate, 1))}
              className="p-1.5 rounded hover:bg-slate-100 text-slate-500"
            >
              <ChevronRight size={15} />
            </button>
          </div>

          {/* Day-of-week headers */}
          <div className="grid grid-cols-7 mb-1">
            {DAYS.map(d => (
              <div key={d} className="text-center text-[10px] font-bold text-slate-400 py-0.5">{d}</div>
            ))}
          </div>

          {/* Day grid */}
          <div className="grid grid-cols-7 gap-y-0.5">
            {Array.from({ length: firstDayOfWeek }).map((_, i) => <div key={`e${i}`} />)}
            {Array.from({ length: daysInMonth }, (_, i) => i + 1).map(day => {
              const isSel = selDay === day && selYear === year && selMonth === month;
              const isToday = today.getDate() === day && today.getMonth() === month && today.getFullYear() === year;
              const disabled = isPastDay(day);
              return (
                <button
                  key={day}
                  type="button"
                  onClick={() => { if (!disabled) handleDayClick(day); }}
                  disabled={disabled}
                  className={`text-xs py-1.5 rounded text-center transition-colors ${
                    disabled
                      ? 'text-slate-300 cursor-not-allowed'
                      : isSel
                      ? 'bg-brand-600 text-white font-bold'
                      : isToday
                      ? 'border border-brand-400 text-brand-700 font-semibold hover:bg-brand-50'
                      : 'text-slate-700 hover:bg-slate-100'
                  }`}
                >
                  {day}
                </button>
              );
            })}
          </div>
        </div>
      )}
    </>
  );
});

export default DatePicker;
