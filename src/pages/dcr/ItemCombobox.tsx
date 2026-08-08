import { useState, useRef, useEffect, useCallback } from 'react';
import { Plus, ChevronDown } from 'lucide-react';
import { fuzzyMatch } from './revenueCalc';

interface ItemType {
  id: number;
  name: string;
  usage_count: number;
  is_system: boolean;
}

interface Props {
  value: string;
  items: ItemType[];
  onChange: (value: string) => void;
  onBlur?: () => void;
  onAddNew: (name: string) => Promise<void>;
  disabled?: boolean;
  placeholder?: string;
  className?: string;
}

export default function ItemCombobox({
  value, items, onChange, onBlur, onAddNew, disabled, placeholder, className,
}: Props) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState(value);
  const [adding, setAdding] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  // Sync query when external value changes (e.g. form reset)
  useEffect(() => { setQuery(value); }, [value]);

  const matches = fuzzyMatch(query, items, 8);
  const exactMatch = items.find(i => i.name.toLowerCase() === query.toLowerCase().trim());
  const showAddNew = query.trim() && !exactMatch;
  const bestMatch = matches[0];
  const closestSuggestion =
    !exactMatch && bestMatch && bestMatch.name.toLowerCase() !== query.toLowerCase().trim()
      ? bestMatch
      : null;

  // Close on outside click
  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (!containerRef.current?.contains(e.target as Node)) {
        setOpen(false);
        if (!exactMatch) {
          onChange(query.trim());
          onBlur?.();
        }
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [open, query, exactMatch, onChange, onBlur]);

  const selectItem = useCallback((name: string) => {
    setQuery(name);
    onChange(name);
    setOpen(false);
    onBlur?.();
  }, [onChange, onBlur]);

  const handleAddNew = useCallback(async () => {
    const name = query.trim().toUpperCase();
    if (!name) return;
    setAdding(true);
    try {
      await onAddNew(name);
      selectItem(name);
    } finally {
      setAdding(false);
    }
  }, [query, onAddNew, selectItem]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Escape') { setOpen(false); inputRef.current?.blur(); }
    if (e.key === 'Enter') {
      e.preventDefault();
      if (matches.length > 0) selectItem(matches[0].name);
      else if (showAddNew) handleAddNew();
    }
    if (e.key === 'Tab') {
      setOpen(false);
      if (!exactMatch) onChange(query.trim());
      onBlur?.();
    }
    if (e.key === 'ArrowDown' && open) {
      e.preventDefault();
      const first = listRef.current?.querySelector<HTMLElement>('[data-idx="0"]');
      first?.focus();
    }
  };

  const handleListKeyDown = (e: React.KeyboardEvent<HTMLElement>, idx: number, name: string) => {
    if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); selectItem(name); }
    if (e.key === 'Escape') { setOpen(false); inputRef.current?.focus(); }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      listRef.current?.querySelector<HTMLElement>(`[data-idx="${idx + 1}"]`)?.focus();
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (idx === 0) inputRef.current?.focus();
      else listRef.current?.querySelector<HTMLElement>(`[data-idx="${idx - 1}"]`)?.focus();
    }
  };

  return (
    <div ref={containerRef} className={`relative ${className ?? ''}`}>
      <div className="relative">
        <input
          ref={inputRef}
          type="text"
          value={query}
          disabled={disabled}
          placeholder={placeholder ?? 'Select or type item…'}
          className="w-full h-full px-2 py-1 pr-7 text-xs border-0 bg-transparent focus:outline-none focus:bg-blue-50 rounded"
          onChange={e => { setQuery(e.target.value); setOpen(true); onChange(e.target.value); }}
          onFocus={() => setOpen(true)}
          onKeyDown={handleKeyDown}
        />
        <button
          type="button"
          tabIndex={-1}
          onClick={() => { setOpen(o => !o); inputRef.current?.focus(); }}
          className="absolute right-1 top-1/2 -translate-y-1/2 text-slate-400 hover:text-slate-600"
        >
          <ChevronDown size={12} />
        </button>
      </div>

      {open && (
        <div
          ref={listRef}
          className="absolute top-full left-0 z-50 bg-white border border-slate-200 rounded-lg shadow-xl min-w-[240px] max-h-64 overflow-y-auto text-xs"
        >
          {/* Closest suggestion hint */}
          {closestSuggestion && (
            <div className="px-3 py-1.5 text-amber-600 bg-amber-50 border-b border-amber-100 text-[11px]">
              Did you mean <strong>{closestSuggestion.name}</strong>?
            </div>
          )}

          {matches.map((item, idx) => (
            <button
              key={item.id}
              data-idx={idx}
              type="button"
              className="w-full flex items-center justify-between px-3 py-1.5 hover:bg-blue-50 focus:bg-blue-50 focus:outline-none text-left gap-2"
              onClick={() => selectItem(item.name)}
              onKeyDown={e => handleListKeyDown(e, idx, item.name)}
            >
              <span className={item.is_system ? 'font-semibold text-slate-700' : 'text-slate-600'}>
                {item.name}
              </span>
              {item.usage_count > 0 && (
                <span className="text-[10px] text-slate-400 shrink-0">×{item.usage_count}</span>
              )}
            </button>
          ))}

          {matches.length === 0 && !showAddNew && (
            <div className="px-3 py-2 text-slate-400">No matches</div>
          )}

          {showAddNew && (
            <button
              type="button"
              disabled={adding}
              className="w-full flex items-center gap-2 px-3 py-2 text-emerald-700 hover:bg-emerald-50 border-t border-slate-100 disabled:opacity-60"
              onClick={handleAddNew}
            >
              <Plus size={12} />
              <span>Add <strong>"{query.trim().toUpperCase()}"</strong> as new item</span>
            </button>
          )}
        </div>
      )}
    </div>
  );
}
