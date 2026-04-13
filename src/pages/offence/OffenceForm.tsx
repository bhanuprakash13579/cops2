/* eslint-disable @typescript-eslint/no-explicit-any */
import { useState, useEffect, useCallback, useMemo, memo, useRef } from 'react';
import { useNavigate, useParams, useLocation } from 'react-router-dom';
import { Save, ArrowLeft, Plus, Trash2, FileText, User, Plane, AlertCircle, FileDigit, CheckCircle, Wand2 } from 'lucide-react';
import DatePicker from '@/components/DatePicker';
import PassportScanner from '@/components/PassportScanner';
import api from '@/lib/api';
import { useRemarksGenerator, detectContextualQuestions, ContextualAnswers, ContextualQuestion } from '@/hooks/useRemarksGenerator';

// ── Static seed list for item-description autocomplete ───────────────────────
// Merged at runtime with DB-fetched suggestions (most-frequent first).
const STATIC_ITEM_SUGGESTIONS = [
  'CIGARETTES','E-CIGARETTES','CIGARS','BIDI','GUTKHA','PAN MASALA','TOBACCO',
  'WHISKY','BRANDY','WINE','BEER','VODKA','RUM','GIN','SCOTCH','CHAMPAGNE','TEQUILA','LIQUOR',
  'GOLD (JEWELLERY)','GOLD (PRIMARY)','GOLD BAR','GOLD BISCUIT','GOLD CHAIN','GOLD RING',
  'SILVER (JEWELLERY)','SILVER BAR',
  'CURRENCY (FOREIGN)','CURRENCY (INDIAN)',
  'MOBILE PHONE','CELL PHONE','IPHONE','LAPTOP','TABLET','SMART WATCH',
  'CAMERA','VIDEO CAMERA','DRONE','ELECTRONIC GOODS',
  'NARCOTICS (CANNABIS/GANJA)','NARCOTICS (HEROIN)','NARCOTICS (COCAINE)',
  'ARMS','AMMUNITION','EXPLOSIVES',
  'ANTIQUES','TOYS','TEXTILES','FABRICS','COSMETICS','PERFUMES','SUNGLASSES','WATCHES',
  'RED SANDERS','POPPY SEEDS','POPPY HUSK',
  'REFURBISHED LAPTOP','REFURBISHED MOBILE PHONE',
  'MARLBORO CIGARETTES','DUNHILL CIGARETTES','GUDANG GARAM CIGARETTES',
  'CHIVAS REGAL WHISKY','JOHNNIE WALKER WHISKY','BARDINET BRANDY','JACK DANIELS WHISKY',
  'MEDICINES','FOOD ITEMS','DRY FRUITS','SPICES','LEATHER GOODS','GARMENTS',
];


const DUTY_TYPES = [
"Antiques-01", "Audio CDs-02", "Cigarettes-03", "Currency (Foreign)-04", "Currency (FICN)-05", 
"Gold (Jewellery)-06", "Gold (Primary)-07", "Liquor-08", "Narcotics (Cannabis/Ganja)-09", 
"Narcotics (Heroin/Brown Sugar)-10", "Narcotics (Cocaine)-11", "Live Species / Wildlife-12", 
"Arms & Ammunition-13", "Silver-14", "Semi Precious / Precious Stones-15", "Video CDs-16", 
"Cameras / Video Cameras-17", "Cell Phones-18", "Cordless Phones-19", "Calculator & Digital Diary-20", 
"Electronic Goods-21", "Miscellaneous-22", "VCD / DVD Players-23", "Walkmans-24", "Watch / Watch Movements-25", 
"Textiles / Fabrics-26", "FEMA (Foreign Exchange)-27", "Commercial Fraud (Imports)-28", 
"Commercial Fraud (Exports)-29", "Tobacco / Gutkha-30", "Morphine-31", "Opium-32", "Psychotropic Substances-33", 
"Ephedrine / Precursors-34", "Fake Indian Goods / IPR-35", "Red Sanders / Timber-36", 
"Ivory / Elephant Products-37", "Pangolin / Animal Parts-38", "Coral / Marine Products-39", 
"Prohibited Imports-40", "Prohibited Exports-41", "Duty Evasion (Imports)-42", "Duty Evasion (Exports)-43", 
"Misdeclaration (Imports)-44", "Misdeclaration (Exports)-45", "Under-valuation (Imports)-46", 
"Under-valuation (Exports)-47", "Overvaluation (Exports)-48", "Drawback Fraud-49", 
"EPCG / Advance Licence Fraud-50", "FTA / Preferential Duty Fraud-51", "Narcotics (Methamphetamine/Synthetic)-52", 
"Narcotics (Ketamine/NPS)-53", "Narcotics (Mandrax/Methaqualone)-54", "Narcotics (Other NDPS)-55", 
"Narcotic (Imports)-56", "Narcotic (Exports)-57", "Explosives-58", "Dual Use / SCOMET Goods-59", 
"Human Trafficking-60", "Hazardous Waste-61", "E-Waste-62", "Areca Nut-63", "Betel Leaves-64", 
"Wildlife (CITES)-65", "ODS (Exports)-66", "ODS (Imports)-67", "Counterfeit Currency-68", 
"Counterfeit Goods-69", "Other_Baggage-99"
];

// Pre-built option elements — created once at module load, never recreated per render
const DUTY_TYPE_OPTIONS = DUTY_TYPES.map(type => (
  <option key={type} value={type}>{type}</option>
));

// Module-level constants — defined once, never reallocated per render

// Maps MRZ ISO 3-letter country codes → display nationality names.
// When a passport is scanned, the MRZ nationality field (e.g. "ESP") is looked up here.
// If not found, the raw code is left as-is so the user can see and correct it manually.
const NATIONALITY_MAP: Record<string, string> = {
  // South Asia
  'IND': 'INDIAN', 'PAK': 'PAKISTANI', 'BGD': 'BANGLADESHI', 'LKA': 'SRI LANKAN',
  'NPL': 'NEPALESE', 'AFG': 'AFGHAN', 'BTN': 'BHUTANESE', 'MDV': 'MALDIVIAN',
  // East Asia
  'CHN': 'CHINESE', 'JPN': 'JAPANESE', 'KOR': 'KOREAN', 'PRK': 'NORTH KOREAN',
  'MNG': 'MONGOLIAN', 'TWN': 'TAIWANESE',
  // Southeast Asia
  'SGP': 'SINGAPOREAN', 'MYS': 'MALAYSIAN', 'VNM': 'VIETNAMESE', 'THA': 'THAI',
  'PHL': 'FILIPINO', 'IDN': 'INDONESIAN', 'MMR': 'MYANMARESE', 'KHM': 'CAMBODIAN',
  'LAO': 'LAOTIAN', 'BRN': 'BRUNEIAN',
  // Middle East
  'ARE': 'UAE', 'SAU': 'SAUDI ARABIAN', 'IRN': 'IRANIAN', 'IRQ': 'IRAQI',
  'JOR': 'JORDANIAN', 'KWT': 'KUWAITI', 'QAT': 'QATARI', 'BHR': 'BAHRAINI',
  'OMN': 'OMANI', 'YEM': 'YEMENI', 'SYR': 'SYRIAN', 'LBN': 'LEBANESE',
  'ISR': 'ISRAELI', 'PSE': 'PALESTINIAN',
  // Anglosphere
  'USA': 'AMERICAN', 'GBR': 'BRITISH', 'CAN': 'CANADIAN', 'AUS': 'AUSTRALIAN',
  'NZL': 'NEW ZEALANDER', 'IRL': 'IRISH',
  // Western Europe
  'FRA': 'FRENCH', 'DEU': 'GERMAN', 'ITA': 'ITALIAN', 'ESP': 'SPANISH',
  'PRT': 'PORTUGUESE', 'NLD': 'DUTCH', 'BEL': 'BELGIAN', 'CHE': 'SWISS',
  'AUT': 'AUSTRIAN', 'SWE': 'SWEDISH', 'NOR': 'NORWEGIAN', 'DNK': 'DANISH',
  'FIN': 'FINNISH', 'GRC': 'GREEK', 'LUX': 'LUXEMBOURGER',
  // Eastern Europe
  'RUS': 'RUSSIAN', 'UKR': 'UKRAINIAN', 'POL': 'POLISH', 'CZE': 'CZECH',
  'SVK': 'SLOVAK', 'HUN': 'HUNGARIAN', 'ROU': 'ROMANIAN', 'BGR': 'BULGARIAN',
  'SRB': 'SERBIAN', 'HRV': 'CROATIAN', 'SVN': 'SLOVENIAN', 'BLR': 'BELARUSIAN',
  // Central Asia & Caucasus
  'TUR': 'TURKISH', 'KAZ': 'KAZAKHSTANI', 'UZB': 'UZBEK', 'TJK': 'TAJIK',
  'TKM': 'TURKMEN', 'KGZ': 'KYRGYZ', 'AZE': 'AZERBAIJANI', 'ARM': 'ARMENIAN', 'GEO': 'GEORGIAN',
  // Africa
  'ZAF': 'SOUTH AFRICAN', 'NGA': 'NIGERIAN', 'KEN': 'KENYAN', 'ETH': 'ETHIOPIAN',
  'EGY': 'EGYPTIAN', 'GHA': 'GHANAIAN', 'TZA': 'TANZANIAN', 'UGA': 'UGANDAN',
  'ZWE': 'ZIMBABWEAN', 'ZMB': 'ZAMBIAN', 'SDN': 'SUDANESE', 'SOM': 'SOMALI',
  'DZA': 'ALGERIAN', 'MAR': 'MOROCCAN', 'TUN': 'TUNISIAN', 'LBY': 'LIBYAN',
  // Americas
  'MEX': 'MEXICAN', 'BRA': 'BRAZILIAN', 'ARG': 'ARGENTINIAN', 'COL': 'COLOMBIAN',
  'CHL': 'CHILEAN', 'PER': 'PERUVIAN', 'VEN': 'VENEZUELAN', 'ECU': 'ECUADORIAN',
  'BOL': 'BOLIVIAN', 'URY': 'URUGUAYAN',
};

// Sorted list used for the nationality datalist (autocomplete suggestions).
// Users can also type any value not in this list — the field is free-text with suggestions.
const NATIONALITY_LIST = [
  'AFGHAN', 'ALGERIAN', 'AMERICAN', 'ARGENTINIAN', 'ARMENIAN', 'AUSTRALIAN', 'AUSTRIAN', 'AZERBAIJANI',
  'BAHRAINI', 'BANGLADESHI', 'BELARUSIAN', 'BELGIAN', 'BHUTANESE', 'BOLIVIAN', 'BRAZILIAN', 'BRITISH', 'BRUNEIAN', 'BULGARIAN',
  'CAMBODIAN', 'CANADIAN', 'CHILEAN', 'CHINESE', 'COLOMBIAN', 'CROATIAN', 'CZECH',
  'DANISH', 'DUTCH',
  'ECUADORIAN', 'EGYPTIAN', 'ETHIOPIAN',
  'FILIPINO', 'FINNISH', 'FRENCH',
  'GEORGIAN', 'GERMAN', 'GHANAIAN', 'GREEK',
  'HUNGARIAN',
  'INDIAN', 'INDONESIAN', 'IRANIAN', 'IRAQI', 'IRISH', 'ISRAELI', 'ITALIAN',
  'JAPANESE', 'JORDANIAN',
  'KAZAKHSTANI', 'KENYAN', 'KOREAN', 'KUWAITI', 'KYRGYZ',
  'LAOTIAN', 'LEBANESE', 'LIBYAN', 'LUXEMBOURGER',
  'MALAYSIAN', 'MALDIVIAN', 'MEXICAN', 'MONGOLIAN', 'MOROCCAN', 'MYANMARESE',
  'NEPALESE', 'NEW ZEALANDER', 'NIGERIAN', 'NORTH KOREAN', 'NORWEGIAN',
  'OMANI',
  'PAKISTANI', 'PALESTINIAN', 'PERUVIAN', 'POLISH', 'PORTUGUESE',
  'QATARI',
  'ROMANIAN', 'RUSSIAN',
  'SAUDI ARABIAN', 'SINGAPOREAN', 'SLOVAK', 'SLOVENIAN', 'SOMALI', 'SOUTH AFRICAN', 'SPANISH', 'SRI LANKAN', 'SUDANESE', 'SWEDISH', 'SWISS', 'SYRIAN',
  'TAJIK', 'TAIWANESE', 'TANZANIAN', 'THAI', 'TUNISIAN', 'TURKISH', 'TURKMEN',
  'UAE', 'UGANDAN', 'UKRAINIAN', 'URUGUAYAN', 'UZBEK',
  'VENEZUELAN', 'VIETNAMESE',
  'YEMENI',
  'ZAMBIAN', 'ZIMBABWEAN',
];

const PORT_MAP: Record<string, string> = {
  // India
  'DEL': 'DELHI', 'BOM': 'MUMBAI', 'MAA': 'CHENNAI', 'CCU': 'KOLKATA', 'BLR': 'BENGALURU',
  'HYD': 'HYDERABAD', 'COK': 'KOCHI', 'AMD': 'AHMEDABAD', 'GOI': 'GOA', 'PNQ': 'PUNE',
  'JAI': 'JAIPUR', 'LKO': 'LUCKNOW', 'TRV': 'TRIVANDRUM', 'IXC': 'CHANDIGARH', 'ATQ': 'AMRITSAR',
  'GAU': 'GUWAHATI', 'PAT': 'PATNA', 'IXR': 'RANCHI', 'VNS': 'VARANASI', 'CCJ': 'CALICUT',
  'SXR': 'SRINAGAR', 'IXM': 'MADURAI', 'TRZ': 'TIRUCHIRAPPALLI', 'IXB': 'BAGDOGRA',
  'VTZ': 'VISAKHAPATNAM', 'NAG': 'NAGPUR', 'IDR': 'INDORE', 'BBI': 'BHUBANESWAR',
  'RPR': 'RAIPUR', 'IXE': 'MANGALORE', 'CJB': 'COIMBATORE', 'UDR': 'UDAIPUR',
  // Middle East
  'DXB': 'DUBAI', 'AUH': 'ABU DHABI', 'SHJ': 'SHARJAH', 'DOH': 'DOHA', 'KWI': 'KUWAIT',
  'JED': 'JEDDAH', 'RUH': 'RIYADH', 'MCT': 'MUSCAT', 'BAH': 'BAHRAIN', 'DMM': 'DAMMAM',
  // Southeast Asia
  'SIN': 'SINGAPORE', 'KUL': 'KUALA LUMPUR', 'BKK': 'BANGKOK', 'DMK': 'BANGKOK (DON MUEANG)',
  'MNL': 'MANILA', 'SGN': 'HO CHI MINH', 'HAN': 'HANOI', 'RGN': 'YANGON', 'PNH': 'PHNOM PENH',
  'CGK': 'JAKARTA', 'DPS': 'BALI', 'MFM': 'MACAU', 'HKG': 'HONG KONG',
  // East Asia
  'NRT': 'TOKYO (NARITA)', 'HND': 'TOKYO (HANEDA)', 'KIX': 'OSAKA', 'ICN': 'SEOUL (INCHEON)',
  'PVG': 'SHANGHAI', 'PEK': 'BEIJING', 'CAN': 'GUANGZHOU', 'TPE': 'TAIPEI',
  // Europe
  'LHR': 'LONDON (HEATHROW)', 'LGW': 'LONDON (GATWICK)', 'CDG': 'PARIS', 'FRA': 'FRANKFURT',
  'AMS': 'AMSTERDAM', 'FCO': 'ROME', 'MXP': 'MILAN', 'MAD': 'MADRID', 'BCN': 'BARCELONA',
  'ZRH': 'ZURICH', 'MUC': 'MUNICH', 'VIE': 'VIENNA', 'IST': 'ISTANBUL',
  // Americas
  'JFK': 'NEW YORK (JFK)', 'EWR': 'NEWARK', 'LAX': 'LOS ANGELES', 'SFO': 'SAN FRANCISCO',
  'ORD': 'CHICAGO', 'YYZ': 'TORONTO', 'GRU': 'SAO PAULO',
  // Africa & Oceania
  'ADD': 'ADDIS ABABA', 'NBO': 'NAIROBI', 'JNB': 'JOHANNESBURG', 'SYD': 'SYDNEY',
  'MEL': 'MELBOURNE', 'AKL': 'AUCKLAND',
  // South Asia
  'DAC': 'DHAKA', 'KTM': 'KATHMANDU', 'CMB': 'COLOMBO', 'MLE': 'MALE', 'MRU': 'MAURITIUS',
  'KHI': 'KARACHI', 'ISB': 'ISLAMABAD', 'LHE': 'LAHORE'
};


const sanitizeInteger = (raw: string) => raw.replace(/[^\d]/g, '');
const sanitizeDecimal = (raw: string) => {
  const cleaned = raw.replace(/[^\d.]/g, '');
  const firstDot = cleaned.indexOf('.');
  const result = firstDot === -1
    ? cleaned
    : cleaned.slice(0, firstDot + 1) + cleaned.slice(firstDot + 1).replace(/\./g, '');
  // Strip leading zeros but preserve "0." for decimals like 0.5
  return result.replace(/^0+([1-9])/, '$1');
};

const numericInputClass = (hasError: boolean, base: string) =>
  `${base} ${hasError ? 'border-red-400 focus:ring-red-500 focus:border-red-500' : ''}`;

const UQC_LABEL: Record<string, string> = {
  NOS: 'Nos.', STK: 'Sticks', KGS: 'Kgs.', GMS: 'Gms.', LTR: 'Ltrs.', MTR: 'Mtrs.', PRS: 'Pairs',
};
const uqcLabel = (code: string) => UQC_LABEL[(code || '').toUpperCase()] ?? (code || 'Nos.');

// ── Item row — memoised so it only re-renders when ITS own data changes ───────
interface ItemRowProps {
  itm: any;
  idx: number;
  rowErrors: Record<string, string> | undefined;
  updateItem: (idx: number, field: string, value: any) => void;
  onRemove: (idx: number) => void;
  onSetFieldError: (idx: number, field: string, message: string) => void;
  onClearFieldError: (idx: number, field: string) => void;
  onDescBlur: (idx: number, desc: string) => void;
  descDatalistId: string;
}

const ItemRow = memo(function ItemRow({ itm, idx, rowErrors, updateItem, onRemove, onSetFieldError, onClearFieldError, onDescBlur, descDatalistId }: ItemRowProps) {
  const [faOpen, setFaOpen] = useState(false);
  const faRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!faOpen) return;
    const handler = (e: MouseEvent) => {
      if (faRef.current && !faRef.current.contains(e.target as Node)) setFaOpen(false);
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [faOpen]);

  const faType = itm.items_fa_type || 'value';
  const faDisplay = faType === 'qty'
    ? (itm.items_fa_qty ? `${itm.items_fa_qty} ${uqcLabel(itm.items_fa_uqc || 'NOS')}` : '—')
    : (Number(itm.items_fa) > 0 ? `₹ ${Number(itm.items_fa).toLocaleString('en-IN')}` : '₹ 0');

  return (
    <tr id={`item-row-${idx}`} className="hover:bg-slate-50 group">
      <td className="px-3 py-1.5 text-center font-medium text-slate-500">{idx + 1}</td>
      <td className="px-2 py-1.5">
        <input
          type="text"
          list={descDatalistId}
          autoComplete="off"
          className={`w-full px-2 py-1 border ${rowErrors?.items_desc ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300'} rounded text-xs uppercase`}
          value={itm.items_desc}
          onChange={e => updateItem(idx, 'items_desc', e.target.value.toUpperCase())}
          onBlur={e => onDescBlur(idx, e.target.value)}
        />
      </td>
      {/* Category: Under OS / Under Duty */}
      <td className="px-2 py-0 align-middle">
        <div className={`flex flex-col justify-center h-[38px] w-full relative`}>
          <select
            className={`w-full px-1.5 py-1 text-[11px] font-semibold rounded border outline-none transition-colors ${
              rowErrors?.items_release_category 
                ? 'border-red-400 ring-1 ring-red-400 text-slate-500 bg-red-50' 
                : 'border-slate-300 text-slate-700 bg-white focus:border-blue-500 focus:ring-1 focus:ring-blue-500'
            }`}
            value={
              itm.items_release_category === 'Under Duty' ? 'Under Duty' :
              itm.items_release_category ? 'Under OS' : ''
            }
            onChange={(e) => updateItem(idx, 'items_release_category', e.target.value)}
          >
            <option value="" disabled>Select</option>
            <option value="Under OS">Under OS</option>
            <option value="Under Duty">Under Duty</option>
          </select>
        </div>
      </td>
      {/* Disposal Category — sub-choice within Under OS */}
      <td className="px-2 py-0 align-middle pl-2">
        {itm.items_release_category === 'Under Duty' || !itm.items_release_category ? (
          <span className="flex items-center justify-start text-xs text-slate-400 italic h-[38px]">N/A</span>
        ) : (
          <div className="flex flex-col justify-center h-[38px]">
            <select
              className={`w-full px-1.5 py-1 text-[11px] font-semibold rounded border outline-none transition-colors ${
                itm.items_release_category === 'Under OS' 
                  ? 'border-red-400 ring-1 ring-red-400 text-slate-500 bg-red-50' 
                  : 'border-slate-300 text-slate-700 bg-white focus:border-blue-500 focus:ring-1 focus:ring-blue-500'
              }`}
              value={!['RF', 'REF', 'CONFS'].includes(itm.items_release_category) ? '' : itm.items_release_category}
              onChange={(e) => updateItem(idx, 'items_release_category', e.target.value)}
            >
              <option value="" disabled>Select Action</option>
              <option value="RF">Redemption</option>
              <option value="REF">Re-export</option>
              <option value="CONFS">Absolute Conf</option>
            </select>
          </div>
        )}
      </td>
      <td className="px-2 py-1.5">
        <select className={`w-full px-2 py-1 border ${rowErrors?.items_duty_type ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300'} rounded text-xs`} value={itm.items_duty_type} onChange={e => updateItem(idx, 'items_duty_type', e.target.value)}>
          {DUTY_TYPE_OPTIONS}
        </select>
      </td>
      <td className="px-2 py-1.5">
        <div className="flex items-center gap-1">
          <input
            type="text"
            inputMode="decimal"
            className={numericInputClass(!!rowErrors?.items_qty, "w-14 px-1.5 py-1 border border-slate-300 rounded text-xs text-center")}
            value={itm.items_qty || ''}
            placeholder="0"
            title={rowErrors?.items_qty || ''}
            onChange={e => {
              const raw = e.target.value;
              const sanitized = sanitizeDecimal(raw);
              updateItem(idx, 'items_qty', sanitized);
              if (!sanitized) { onSetFieldError(idx, 'items_qty', 'Quantity required'); return; }
              if (sanitized !== raw) { onSetFieldError(idx, 'items_qty', 'Numbers only'); return; }
              onClearFieldError(idx, 'items_qty');
            }}
          />
          <select className={`w-16 px-1 py-1 border ${rowErrors?.items_uqc ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300'} rounded text-[10px]`} value={itm.items_uqc} onChange={e => updateItem(idx, 'items_uqc', e.target.value)}>
            <option value="NOS">Nos.</option>
            <option value="STK">Sticks</option>
            <option value="KGS">Kgs.</option>
            <option value="GMS">Gms.</option>
            <option value="LTR">Ltrs.</option>
            <option value="MTR">Mtrs.</option>
            <option value="PRS">Pairs</option>
          </select>
        </div>
      </td>
      <td className="px-2 py-1.5">
        <input
          type="text"
          inputMode="decimal"
          className={numericInputClass(!!rowErrors?.value_per_piece, "w-full px-2 py-1 border border-slate-300 rounded text-xs text-right")}
          value={itm.value_per_piece || ''}
          placeholder="0"
          title={rowErrors?.value_per_piece || ''}
          onChange={e => {
            const raw = e.target.value;
            const sanitized = sanitizeDecimal(raw);
            updateItem(idx, 'value_per_piece', sanitized);
            if (sanitized !== raw) { onSetFieldError(idx, 'value_per_piece', 'Numbers only'); return; }
            onClearFieldError(idx, 'value_per_piece');
          }}
        />
      </td>
      {/* Free Allowance — moved BEFORE "Value after FA" */}
      <td className="px-2 py-1.5 relative">
        {itm.items_release_category === 'CONFS' || !itm.items_release_category ? (
          <span className="flex items-center justify-center text-xs text-slate-400 italic">N/A</span>
        ) : (
          <div ref={faRef} className="relative">
            {/* Compact pill — click to open editor */}
            <button
              type="button"
              onClick={() => setFaOpen(o => !o)}
              className="w-full px-2 py-1 text-xs text-right border border-slate-300 rounded bg-white hover:border-blue-400 hover:bg-blue-50 transition-colors truncate"
              title="Click to set Free Allowance"
            >
              {faDisplay}
            </button>
            {/* Floating editor panel */}
            {faOpen && (
              <div className="absolute z-50 top-full mt-1 right-0 bg-white border border-slate-300 rounded-lg shadow-xl p-2 flex items-center gap-1 min-w-max">
                <select
                  value={faType}
                  onChange={e => updateItem(idx, 'items_fa_type', e.target.value)}
                  className="shrink-0 w-10 py-1 text-[9px] border border-slate-300 rounded bg-slate-50 text-center cursor-pointer focus:outline-none"
                  title="FA type: ₹ = monetary value, Qty = quantity allowed free"
                >
                  <option value="value">₹</option>
                  <option value="qty">Qty</option>
                </select>
                {faType === 'qty' ? (
                  <>
                    <input
                      type="text" inputMode="decimal"
                      value={itm.items_fa_qty || ''}
                      placeholder="0"
                      autoFocus
                      onFocus={e => e.target.select()}
                      onChange={e => updateItem(idx, 'items_fa_qty', sanitizeDecimal(e.target.value))}
                      className="w-14 py-1 px-1 border border-slate-300 rounded text-xs text-right focus:outline-none focus:ring-1 focus:ring-blue-400"
                    />
                    <span
                      className="w-12 py-1 px-1 border border-slate-300 rounded text-[10px] bg-slate-100 text-slate-600 text-center"
                      title="Unit is locked to match the item's quantity unit"
                    >{uqcLabel(itm.items_uqc || 'NOS')}</span>
                  </>
                ) : (
                  <input
                    type="text" inputMode="decimal"
                    value={itm.items_fa || ''}
                    placeholder="0"
                    autoFocus
                    onFocus={e => e.target.select()}
                    title="Free Allowance in ₹ — deducted before duty calculation"
                    onChange={e => updateItem(idx, 'items_fa', sanitizeDecimal(e.target.value))}
                    className="w-24 py-1 px-1 border border-slate-300 rounded text-xs text-right focus:outline-none focus:ring-1 focus:ring-blue-400"
                  />
                )}
              </div>
            )}
          </div>
        )}
      </td>
      {/* Value after FA — shows total value minus the effective free allowance deduction */}
      <td className="px-2 py-1.5 bg-amber-50 text-right font-bold text-amber-900 text-xs">
        {(() => {
          const totalVal = Number(itm.items_value || 0);
          if (itm.items_release_category === 'CONFS' || !itm.items_release_category) {
            return `₹ ${totalVal.toLocaleString('en-IN', { maximumFractionDigits: 2 })}`;
          }
          const faT = itm.items_fa_type || 'value';
          let effectiveFa = 0;
          if (faT === 'qty') {
            const tq = Number(itm.items_qty || 0);
            const fq = Number(itm.items_fa_qty || 0);
            const remainQty = Math.max(0, tq - fq);
            const vpp = Number(itm.value_per_piece || 0);
            return `₹ ${(remainQty * vpp).toLocaleString('en-IN', { maximumFractionDigits: 2 })}`;
          } else {
            effectiveFa = Math.min(Number(itm.items_fa || 0), totalVal);
            return `₹ ${Math.max(0, totalVal - effectiveFa).toLocaleString('en-IN', { maximumFractionDigits: 2 })}`;
          }
        })()}
      </td>
      <td className="px-2 py-1.5">
        {itm.items_release_category === 'CONFS' ? (
          <span className="flex items-center justify-center w-full px-2 py-1 text-xs text-slate-400 italic h-[26px]">N/A</span>
        ) : (
          <input
            type="text"
            inputMode="numeric"
            className={numericInputClass(!!rowErrors?.cumulative_duty_rate, "w-full px-2 py-1 border border-slate-300 rounded text-xs text-center font-bold")}
            value={itm.cumulative_duty_rate || ''}
            placeholder="0"
            title={rowErrors?.cumulative_duty_rate || ''}
            onChange={e => {
              const raw = e.target.value;
              const sanitized = sanitizeInteger(raw);
              updateItem(idx, 'cumulative_duty_rate', sanitized);
              if (!sanitized) { onSetFieldError(idx, 'cumulative_duty_rate', 'Required'); return; }
              if (sanitized !== raw) { onSetFieldError(idx, 'cumulative_duty_rate', 'Digits only'); return; }
              onClearFieldError(idx, 'cumulative_duty_rate');
            }}
          />
        )}
      </td>
      <td className={`px-2 py-1.5 text-right font-bold text-sm h-[38px] ${itm.items_release_category === 'CONFS' ? 'bg-red-50 text-red-400 italic' : 'bg-brand-50 text-brand-700'}`}>
        {itm.items_release_category === 'CONFS' ? 'N/A' : `₹ ${Number(itm.items_duty || 0).toLocaleString('en-IN', { maximumFractionDigits: 0 })}`}
      </td>
      <td className="px-2 py-1.5 text-center">
        <button onClick={(e) => { e.preventDefault(); onRemove(idx); }} className="text-slate-400 hover:text-red-500">
          <Trash2 size={16} />
        </button>
      </td>
    </tr>
  );
});

// Module-level cache — item descriptions are fetched once per session
let _descSuggestionsCache: string[] | null = null;

// ─────────────────────────────────────────────────────────────────────────────
export default function OffenceForm() {
  const { osNo, osYear } = useParams();
  const navigate = useNavigate();
  const location = useLocation();
  const { generateRemark, loading: remarksLoading } = useRemarksGenerator();

  // ── Item-description autocomplete suggestions ────────────────────────────
  const [descSuggestions, setDescSuggestions] = useState<string[]>(_descSuggestionsCache ?? STATIC_ITEM_SUGGESTIONS);
  useEffect(() => {
    if (_descSuggestionsCache !== null) return; // already fetched this session
    api.get('/os/item-descriptions')
      .then(res => {
        if (!Array.isArray(res.data)) return;
        const dbItems: string[] = res.data.map((s: string) => (s || '').toUpperCase()).filter(Boolean);
        const merged = [...dbItems];
        const existing = new Set(dbItems);
        for (const s of STATIC_ITEM_SUGGESTIONS) { if (!existing.has(s)) { merged.push(s); existing.add(s); } }
        _descSuggestionsCache = merged;
        setDescSuggestions(merged);
      })
      .catch(() => { /* keep static list on error */ });
  }, []);
  // Memoize the datalist so 300 <option> elements don't re-render on every formData keystroke
  const descDatalist = useMemo(() => (
    <datalist id="item-desc-datalist">
      {descSuggestions.map(s => <option key={s} value={s} />)}
    </datalist>
  ), [descSuggestions]);

  // ── Contextual questions modal ────────────────────────────────────────────
  const [showContextModal, setShowContextModal] = useState(false);
  const [contextQuestions, setContextQuestions] = useState<ContextualQuestion[]>([]);
  const [contextAnswers, setContextAnswers] = useState<ContextualAnswers>({});
  const [pendingRole, setPendingRole] = useState<'SUPDT' | 'ADJN'>('SUPDT');

  const triggerGenerateRemarks = (role: 'SUPDT' | 'ADJN') => {
    const questions = detectContextualQuestions(items);
    if (questions.length > 0) {
      setPendingRole(role);
      setContextQuestions(questions);
      setContextAnswers({});
      setShowContextModal(true);
    } else {
      const text = generateRemark(role, items, {
        pax_name: formData.pax_name, flight_no: formData.flight_no,
        flight_date: formData.flight_date, port_of_dep_dest: formData.port_of_dep_dest,
        os_date: formData.os_date, passport_no: formData.passport_no,
        passport_date: formData.passport_date, case_type: formData.case_type,
      }, {});
      setSupdtsRemarks(text);
    }
  };

  const handleContextSubmit = () => {
    const text = generateRemark(pendingRole, items, {
      pax_name: formData.pax_name, flight_no: formData.flight_no,
      flight_date: formData.flight_date, port_of_dep_dest: formData.port_of_dep_dest,
      os_date: formData.os_date, passport_no: formData.passport_no,
      passport_date: formData.passport_date, case_type: formData.case_type,
    }, contextAnswers);
    setSupdtsRemarks(text);
    setShowContextModal(false);
  };

  const isEditing = !!osNo;
  const isViewOnly = location.pathname.endsWith('/view');
  // When the form is opened from inside the adjudication module (edit-sdo route),
  // navigate back to the adjudication case form instead of the SDO list.
  const isInAdjModule = location.pathname.startsWith('/adjudication');
  const goBackPath = isInAdjModule
    ? `/adjudication/case/${osNo}/${osYear}`
    : '/sdo/offence';

  const [formData, setFormData] = useState({
    os_no: '',
    os_date: new Date().toISOString().split('T')[0],
    shift: 'Day',
    booked_by: 'Batch A',
    case_type: 'Non-Bonafide',
    detention_date: new Date().toISOString().split('T')[0],
    
    pax_name: '',
    father_name: 'S/o D/o W/o ',
    pax_nationality: 'INDIAN',
    passport_no: '',
    passport_date: '',
    pp_issue_place: '',
    old_passport_no: '',
    
    pax_address1: '',
    pax_address2: '',
    pax_address3: '',
    pax_date_of_birth: '',
    residence_at: 'INDIA',
    
    port_of_dep_dest: '',
    arrived_from: 'Others', // mapped to country_of_departure 
    date_of_departure: 'N.A.',
    stay_abroad_days: '',
    flight_no: '',
    flight_date: new Date().toISOString().split('T')[0],
    
    previous_os_details: '',
    previous_visits: '',
    adjn_offr_remarks: '',
    pax_status: '',

    is_draft: 'Y',
    dr_no: '',
    dr_year: new Date().getFullYear(),
    
    rf_amount: 0,
    pp_amount: 0,
    ref_amount: 0,
    br_amount: 0,
  });

  const [items, setItems] = useState<any[]>([{
      items_desc: '',
      items_qty: 1,
      items_uqc: 'NOS',
      value_per_piece: 0,
      items_value: 0,
      items_fa: 0,
      items_fa_type: 'value',
      items_fa_qty: 0,
      items_fa_uqc: 'NOS',
      cumulative_duty_rate: 35,
      items_duty: 0,
      items_release_category: '',
      items_duty_type: 'Miscellaneous-22'
  }]);
  
  // Separate state for supdts_remarks — keeps it isolated so typing in the
  // remarks textarea doesn't trigger a full re-render of the entire form.
  const [supdtsRemarks, setSupdtsRemarks] = useState('');

  const [errorMsg, setErrorMsg] = useState('');
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [oldPassportsSuggestion, setOldPassportsSuggestion] = useState<string[]>([]);
  const [showPassportAlert, setShowPassportAlert] = useState(false);
  const [showExitModal, setShowExitModal] = useState(false);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});
  const [itemErrors, setItemErrors] = useState<Record<number, Record<string, string>>>({});
  const osNoCheckTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  const setItemFieldError = useCallback((idx: number, field: string, message: string) => {
    setItemErrors(prev => ({
      ...prev,
      [idx]: { ...(prev[idx] || {}), [field]: message },
    }));
  }, []);

  const clearItemFieldError = useCallback((idx: number, field: string) => {
    setItemErrors(prev => {
      const current = prev[idx];
      if (!current || !current[field]) return prev;
      // eslint-disable-next-line @typescript-eslint/no-unused-vars
      const { [field]: _removed, ...rest } = current;
      const next = { ...prev, [idx]: rest };
      if (Object.keys(rest).length === 0) {
        // eslint-disable-next-line @typescript-eslint/no-unused-vars
        const { [idx]: _idxRemoved, ...withoutIdx } = next;
        return withoutIdx;
      }
      return next;
    });
  }, []);

  const setFieldError = useCallback((field: string, message: string) => {
    setFieldErrors(prev => ({ ...prev, [field]: message }));
  }, []);

  const clearFieldError = useCallback((field: string) => {
    setFieldErrors(prev => {
      if (!prev[field]) return prev;
      const { [field]: _removed, ...rest } = prev;
      return rest;
    });
  }, []);

  useEffect(() => {
    if (isEditing && osNo && osYear) {
      api.get(`/os/${osNo}/${osYear}`)
      .then(res => {
        const data = res.data;
        const safeData = { ...formData };
        Object.keys(safeData).forEach(key => {
            if (data[key] !== undefined && data[key] !== null) {
                (safeData as Record<string, unknown>)[key] = data[key];
            }
        });
        // Map country_of_departure → arrived_from. The non-export select only has four valid
        // options; anything else (e.g. a destination stored when the case was an Export Case)
        // is normalised to 'Others' so the select never renders blank.
        if (data.country_of_departure) {
          const validArrivedFrom = ['Nepal', 'Bhutan', 'Myanmar', 'Others'];
          safeData.arrived_from = validArrivedFrom.includes(data.country_of_departure)
            ? data.country_of_departure
            : 'Others';
        }
        
        setFormData(safeData);
        setSupdtsRemarks(data.supdts_remarks || '');
        if (data.items && data.items.length > 0) setItems(data.items);
      })
      .catch(err => setErrorMsg(err.message));
    }
  }, [isEditing, osNo, osYear]);

  // Auto-calculate Stay Abroad Days — bail early if already correct to avoid spurious re-renders
  useEffect(() => {
    const setDays = (val: string) =>
      setFormData(prev => prev.stay_abroad_days === val ? prev : { ...prev, stay_abroad_days: val });

    if (formData.flight_date && formData.date_of_departure && formData.date_of_departure !== 'N.A.' && formData.date_of_departure !== 'NA') {
      try {
        const arr = new Date(formData.flight_date);
        const dep = new Date(formData.date_of_departure);
        if (!isNaN(arr.getTime()) && !isNaN(dep.getTime())) {
          const diffDays = (arr.getTime() - dep.getTime()) / (1000 * 3600 * 24);
          setDays(diffDays >= 0 ? Math.round(diffDays).toString() : '');
        } else {
          setDays('');
        }
      } catch {
        setDays('');
      }
    } else {
      setDays('');
    }
  }, [formData.flight_date, formData.date_of_departure]);

  // Search Old Passports on Name + DOB change
  useEffect(() => {
    if (formData.pax_name.length <= 3 || !formData.pax_date_of_birth) return;
    const ctrl = new AbortController();
    const timer = setTimeout(async () => {
      try {
        const res = await api.post('/passports/search', {
          name: formData.pax_name, dob: formData.pax_date_of_birth,
        }, { signal: ctrl.signal });
        const data = res.data;
        if (data.passports && data.passports.length > 0) {
          const existing = (formData.old_passport_no || '').split(';').map((s: string) => s.trim()).filter(Boolean);
          const newSuggestions = data.passports.filter((p: string) => !existing.includes(p) && p !== formData.passport_no);
          if (newSuggestions.length > 0) {
            setOldPassportsSuggestion(newSuggestions);
            setShowPassportAlert(true);
          }
        }
      } catch { /* aborted or network error — silent */ }
    }, 1000);
    return () => { clearTimeout(timer); ctrl.abort(); };
  }, [formData.pax_name, formData.pax_date_of_birth, formData.passport_no]);

  const acceptPassportSuggestion = () => {
      setFormData(prev => ({
          ...prev, 
          old_passport_no: prev.old_passport_no 
              ? `${prev.old_passport_no}; ${oldPassportsSuggestion.join('; ')}`
              : oldPassportsSuggestion.join('; ')
      }));
      setShowPassportAlert(false);
      setOldPassportsSuggestion([]);
  };

  // Memoised so the tfoot total doesn't recompute on every formData keystroke
  const totalDuty = useMemo(
    () => items.reduce((acc, itm) => acc + Number(itm.items_duty || 0), 0),
    [items]
  );

  // Sum of "Value after FA" column — mirrors the per-row deduction logic
  const totalValueAfterFA = useMemo(() => {
    return items.reduce((acc, itm) => {
      const totalVal = Number(itm.items_value || 0);
      if (itm.items_release_category === 'CONFS' || !itm.items_release_category) {
        return acc + totalVal;
      }
      const faT = itm.items_fa_type || 'value';
      if (faT === 'qty') {
        const tq = Number(itm.items_qty || 0);
        const fq = Number(itm.items_fa_qty || 0);
        const vpp = Number(itm.value_per_piece || 0);
        return acc + Math.max(0, tq - fq) * vpp;
      } else {
        const fa = Math.min(Number(itm.items_fa || 0), totalVal);
        return acc + Math.max(0, totalVal - fa);
      }
    }, 0);
  }, [items]);

  // Stable remove handler — doesn't need items in closure
  const onRemove = useCallback((idx: number) => {
    setItems(prev => prev.filter((_, i) => i !== idx));
  }, []);

  // Smart classification — runs every time the description field loses focus.
  // AbortController cancels any in-flight request from a prior blur on the same row.
  const classifyAbortRefs = useRef<Record<number, AbortController>>({});
  // Abort all pending classify calls on unmount
  useEffect(() => () => { Object.values(classifyAbortRefs.current).forEach(c => c.abort()); }, []);

  // Ctrl+S / Cmd+S → save as draft without leaving the page
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 's') {
        e.preventDefault();
        submitData('Y');
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, []);  // eslint-disable-line react-hooks/exhaustive-deps

  const onDescBlur = useCallback(async (idx: number, desc: string) => {
    if (!desc || desc.trim().length < 3) return;
    // Cancel previous in-flight classify for this row
    classifyAbortRefs.current[idx]?.abort();
    const ctrl = new AbortController();
    classifyAbortRefs.current[idx] = ctrl;
    try {
      const res = await api.get('/os/classify-item', { params: { description: desc }, signal: ctrl.signal });
      if (res.data?.duty_type && res.data.duty_type !== 'Miscellaneous-22') {
        setItems(prev => {
          const row = prev[idx];
          if (!row) return prev;
          const updated = { ...row, items_duty_type: res.data.duty_type };
          if (res.data.uqc && res.data.uqc !== 'NOS') {
            updated.items_uqc = res.data.uqc;
          }
          return prev.map((item, i) => i === idx ? updated : item);
        });
      }
    } catch { /* silent — classification is best-effort, AbortError included */ }
  }, []);

  // Stable updateItem — uses functional setItems so no stale closure on items
  const updateItem = useCallback((idx: number, field: string, value: any) => {
    setItems(prevItems => {
      // Create a fresh shallow copy of the changed row only; leave others as-is
      const newItems = prevItems.map((item, i) => i === idx ? { ...item, [field]: value } : item);

      // If switching category, zero or restore duty
      if (field === 'items_release_category') {
          if (value === 'CONFS' || value === 'REF') {
              // Absolute confiscation & Re-Export — zero FA fields, duty, and rate
              newItems[idx] = { ...newItems[idx],
                  items_fa: 0, items_fa_type: 'value', items_fa_qty: 0, items_fa_uqc: 'NOS',
                  cumulative_duty_rate: 0, items_duty: 0 };
          } else if ((value === 'Under Duty' || value === 'RF' || value === 'Under OS') && newItems[idx].cumulative_duty_rate === 0) {
              // Restore default duty rate when switching to a dutiable category from a zeroed state
              const row = newItems[idx];
              const val = Number(row.items_value || 0);
              let fa = 0;
              if ((row.items_fa_type || 'value') === 'qty') {
                  const tq = Number(row.items_qty || 0);
                  const fq = Number(row.items_fa_qty || 0);
                  fa = tq > 0 ? Math.min((fq / tq) * val, val) : 0;
              } else {
                  fa = Number(row.items_fa || 0);
              }
              const dutiableVal = Math.max(0, val - fa);
              newItems[idx] = { ...newItems[idx], cumulative_duty_rate: 35, items_duty: (dutiableVal * 35) / 100 };
          }
      }

      // When FA type switches, clear the other mode's fields
      if (field === 'items_fa_type') {
          if (value === 'qty') {
              // Auto-inherit the item's unit so mismatch (e.g. GMS on a LTR item) is impossible
              newItems[idx] = { ...newItems[idx], items_fa: 0, items_fa_uqc: newItems[idx].items_uqc || 'NOS' };
          } else {
              newItems[idx] = { ...newItems[idx], items_fa_qty: 0, items_fa_uqc: 'NOS' };
          }
      }

      // When item UQC changes and FA is in qty mode, keep fa_uqc in sync
      if (field === 'items_uqc' && (newItems[idx].items_fa_type || 'value') === 'qty') {
          newItems[idx] = { ...newItems[idx], items_fa_uqc: value };
      }

      const isNoDuty = newItems[idx].items_release_category === 'CONFS' || newItems[idx].items_release_category === 'REF';

      // FA deducted for dutiable goods
      const recalcDuty = (item: any) => {
          const val = Number(item.items_value || 0);
          let fa = 0;
          if (['Under Duty', 'Under OS', 'RF'].includes(item.items_release_category)) {
              if ((item.items_fa_type || 'value') === 'qty') {
                  // Proportional: (fa_qty / total_qty) × total_value
                  const totalQty = Number(item.items_qty || 0);
                  const faQty = Number(item.items_fa_qty || 0);
                  fa = totalQty > 0 ? Math.min((faQty / totalQty) * val, val) : 0;
              } else {
                  fa = Number(item.items_fa || 0);
              }
          }
          const rate = Number(item.cumulative_duty_rate || 0);
          return (Math.max(0, val - fa) * rate) / 100;
      };

      if (!isNoDuty) {
          if (field === 'value_per_piece' || field === 'items_qty') {
              const qty = field === 'items_qty' ? Number(value || 0) : Number(newItems[idx].items_qty || 0);
              const vpp = field === 'value_per_piece' ? Number(value || 0) : Number(newItems[idx].value_per_piece || 0);
              const items_value = qty * vpp;
              const items_fa = Number(newItems[idx].items_fa || 0) > items_value ? items_value : Number(newItems[idx].items_fa || 0);
              newItems[idx] = { ...newItems[idx], items_value, items_fa, items_duty: recalcDuty({ ...newItems[idx], items_value, items_fa }) };
          }
          if (field === 'items_value' || field === 'cumulative_duty_rate' || field === 'items_fa' || field === 'items_fa_qty' || field === 'items_fa_type' || field === 'items_release_category') {
              if (field === 'items_fa') {
                  const itemVal = Number(newItems[idx].items_value || 0);
                  if (Number(value || 0) > itemVal) {
                      newItems[idx] = { ...newItems[idx], items_fa: itemVal };
                  }
              }
              if (field === 'items_fa_qty') {
                  const totalQty = Number(newItems[idx].items_qty || 0);
                  if (Number(value || 0) > totalQty) {
                      newItems[idx] = { ...newItems[idx], items_fa_qty: totalQty };
                  }
              }
              newItems[idx] = { ...newItems[idx], items_duty: recalcDuty(newItems[idx]) };
          }
      } else {
          const qty = field === 'items_qty' ? Number(value || 0) : Number(newItems[idx].items_qty || 0);
          const vpp = field === 'value_per_piece' ? Number(value || 0) : Number(newItems[idx].value_per_piece || 0);
          newItems[idx] = { ...newItems[idx], items_value: qty * vpp, items_duty: 0, cumulative_duty_rate: 0, items_fa: 0 };
      }
      return newItems;
    });
  }, []);

  const submitData = async (draftValue: string) => {
    setErrorMsg('');
    setFieldErrors({});
    setItemErrors({});

    // Guided validation only for Save & Exit (not for drafts)
    if (draftValue === 'N') {
      const errors: Record<string, string> = {};
      const itemErrs: Record<number, Record<string, string>> = {};

      const requireField = (key: keyof typeof formData, label: string, fallback?: string) => {
        const val = String(formData[key] || fallback || '').trim();
        if (!val) errors[String(key)] = `${label} is required.`;
      };

      // Header / case-level required fields
      requireField('os_no', 'O.S. No.');
      requireField('os_date', 'O.S. Date');
      requireField('shift', 'Shift', 'Day');
      requireField('booked_by', 'Booked By', 'Batch A');
      requireField('case_type', 'Case Type', 'Non-Bonafide');
      requireField('detention_date', 'Detention/Seizure Date');

      // Passenger / passport
      requireField('pax_name', 'Passenger Name');
      requireField('pax_date_of_birth', 'Date of Birth');
      requireField('pax_nationality', 'Nationality', 'INDIAN');
      requireField('passport_no', 'Passport No.');
      requireField('passport_date', 'Passport Date (Expiry)');
      requireField('pp_issue_place', 'Place Of Issue Of PP');
      requireField('pax_address1', 'Passenger Address');
      requireField('residence_at', 'Normal Residence At', 'INDIA');

      // Travel
      requireField('flight_no', 'Flight No.');
      requireField('flight_date', 'Flight Date');
      requireField('port_of_dep_dest', 'Port of Dep/Dest');
      const isExportCase = formData.case_type === 'Export Case';
      if (!isExportCase) {
        requireField('arrived_from', 'Arrived From');
        // Date of departure: allow "N.A." or a non-empty date string
        if (!String(formData.date_of_departure ?? '').trim()) {
          errors['date_of_departure'] = 'Date of Departure from India is required (or N.A.).';
        }
      }

      // History / remarks
      requireField('previous_visits', 'Previous Visits');
      if (!supdtsRemarks.trim()) errors['supdts_remarks'] = "Supdt's Remarks is required.";
      if (supdtsRemarks.length > 1500) errors['supdts_remarks'] = "Supdt's Remarks exceeds maximum limit of 1500 characters.";

      // O.S. No format
      if (!errors.os_no) {
        const osVal = String(formData.os_no).trim();
        if (!/^\d+$/.test(osVal)) {
          errors.os_no = 'O.S. No. must be a number (digits only).';
        }
      }

      // At least one item row
      if (!items || items.length === 0) {
        errors['items'] = 'At least one seized goods item is required.';
      }

      // Item-level required fields
      items.forEach((itm, idx) => {
        const rowErrors: Record<string, string> = {};
        
        const desc = itm.items_desc;
        if (desc === undefined || desc === null || String(desc).trim() === '') rowErrors.items_desc = 'Description is required.';
        
        const qty = itm.items_qty;
        if (qty === undefined || qty === null || String(qty).trim() === '') rowErrors.items_qty = 'Quantity required.';
        
        const uqc = itm.items_uqc;
        if (uqc === undefined || uqc === null || String(uqc).trim() === '') rowErrors.items_uqc = 'UQC required.';
        
        const val = itm.items_value;
        const vpp = itm.value_per_piece;
        const hasVal = val !== undefined && val !== null && String(val).trim() !== '';
        const hasVpp = vpp !== undefined && vpp !== null && String(vpp).trim() !== '';
        if (!hasVal && !hasVpp) {
          rowErrors.items_value = 'Enter total value or value per piece.';
        }
        
        const isNoDuty = itm.items_release_category === 'CONFS' || itm.items_release_category === 'REF';
        const rateVal = itm.cumulative_duty_rate;
        if (!isNoDuty && (rateVal === undefined || rateVal === null || String(rateVal).trim() === '')) {
          rowErrors.cumulative_duty_rate = 'Duty rate required.';
        }
        
        const dutyType = itm.items_duty_type;
        if (dutyType === undefined || dutyType === null || String(dutyType).trim() === '') {
          rowErrors.items_duty_type = 'Duty Type required.';
        }
        
        const relCat = itm.items_release_category;
        if (!relCat || !['Under OS', 'Under Duty', 'CONFS', 'RF', 'REF'].includes(relCat)) {
          rowErrors.items_release_category = 'Select Category';
        }
        if (Object.keys(rowErrors).length > 0) {
          itemErrs[idx] = rowErrors;
        }
      });

      if (Object.keys(errors).length > 0 || Object.keys(itemErrs).length > 0) {
        setFieldErrors(errors);
        setItemErrors(itemErrs);
        setErrorMsg('Please fill all mandatory fields highlighted in red before saving.');

        // Scroll to first error field or first erroneous item row
        const firstFieldKey = Object.keys(errors)[0];
        if (firstFieldKey && firstFieldKey !== 'items') {
          const el = document.getElementById(`field-${firstFieldKey}`);
          if (el) {
            el.scrollIntoView({ behavior: 'smooth', block: 'center' });
            (el as HTMLElement & { focus?: () => void }).focus?.();
          }
        } else if (Object.keys(itemErrs).length > 0) {
          const firstRow = Number(Object.keys(itemErrs)[0]);
          const el = document.getElementById(`item-row-${firstRow}`);
          if (el) {
            el.scrollIntoView({ behavior: 'smooth', block: 'center' });
          }
        }
        return;
      }
    }

    setIsSubmitting(true);

    try {
        const payload = {
            ...formData,
            supdts_remarks: supdtsRemarks,
            shift: formData.shift || 'Day',
            booked_by: formData.booked_by || 'Batch A',
            case_type: formData.case_type || 'Non-Bonafide',
            pax_nationality: formData.pax_nationality || 'INDIAN',
            residence_at: formData.residence_at || 'INDIA',
            country_of_departure: formData.arrived_from,
            is_draft: draftValue,
            items: items.map((itm, idx) => ({
                items_sno: idx + 1,
                items_desc: itm.items_desc || '',
                items_qty: Number(itm.items_qty || 0),
                items_uqc: itm.items_uqc || 'NOS',
                value_per_piece: Number(itm.value_per_piece || 0),
                items_value: Number(itm.items_value || 0),
                cumulative_duty_rate: Number(itm.cumulative_duty_rate || 0),
                items_duty: Number(itm.items_duty || 0),
                items_fa: Number(itm.items_fa || 0),
                items_fa_type: itm.items_fa_type || 'value',
                items_fa_qty: Number(itm.items_fa_qty || 0),
                items_fa_uqc: itm.items_fa_uqc || 'NOS',
                items_duty_type: itm.items_duty_type || '',
                items_category: itm.items_release_category || 'Under OS',
                items_release_category: itm.items_release_category || '',
                items_sub_category: '',
                items_dr_no: Number(formData.dr_no || 0),
                items_dr_year: Number(formData.dr_year || new Date().getFullYear())
            }))
        };

        const finalPayload: any = { ...payload };
        const isDateValid = (d: string) => d && /^\d{4}-\d{2}-\d{2}$/.test(d);
        if (!isDateValid(finalPayload.passport_date)) delete finalPayload.passport_date;
        if (!isDateValid(finalPayload.pax_date_of_birth)) delete finalPayload.pax_date_of_birth;
        if (!isDateValid(finalPayload.flight_date)) delete finalPayload.flight_date;
        if (!isDateValid(finalPayload.detention_date)) delete finalPayload.detention_date;
        if (!isDateValid(finalPayload.os_date)) delete finalPayload.os_date;
        if (finalPayload.date_of_departure && finalPayload.date_of_departure !== 'N.A.' && finalPayload.date_of_departure !== 'NA' && !isDateValid(finalPayload.date_of_departure)) {
            delete finalPayload.date_of_departure;
        }
        if (!finalPayload.stay_abroad_days || isNaN(parseInt(finalPayload.stay_abroad_days))) {
            delete finalPayload.stay_abroad_days;
        }

        await (isEditing
            ? api.put(`/os/${osNo}/${osYear}`, finalPayload)
            : api.post('/os', finalPayload));

        navigate(goBackPath);

    } catch(err: any) {
        let errMsg = err.response?.data?.detail || err.message || 'Failed to save offence case';
        if (Array.isArray(errMsg)) {
            errMsg = errMsg.map((e: any) => `${e.loc?.join('.')} - ${e.msg}`).join(', ');
        } else if (typeof errMsg === 'object') {
            errMsg = JSON.stringify(errMsg);
        }
        setErrorMsg(errMsg);
    } finally {
        setIsSubmitting(false);
    }
  };

  const handleScan = useCallback((scanData: any) => {
    if (scanData.type === 'PASSPORT') {
        let mappedNationality = scanData.nationality?.toUpperCase();
        if (mappedNationality && NATIONALITY_MAP[mappedNationality]) {
            mappedNationality = NATIONALITY_MAP[mappedNationality];
        }

        const fatherPrefix = scanData.gender === 'M' ? 'S/O ' : scanData.gender === 'F' ? 'D/O W/O ' : 'S/O D/O W/O ';

        setFormData(prev => ({
            ...prev,
            pax_name: scanData.fullName,
            passport_no: scanData.passportNo,
            pax_nationality: mappedNationality || prev.pax_nationality,
            pax_date_of_birth: scanData.dateOfBirth,
            passport_date: scanData.expiryDate || prev.passport_date,
            father_name: fatherPrefix,
        }));
    } else if (scanData.type === 'BOARDING_PASS') {
        const originCode = scanData.origin?.toUpperCase()?.trim();
        const destCode = scanData.destination?.toUpperCase()?.trim();
        const mappedOrigin = (originCode && PORT_MAP[originCode]) ? PORT_MAP[originCode] : originCode;
        const mappedDest = (destCode && PORT_MAP[destCode]) ? PORT_MAP[destCode] : destCode;

        // Combine origin and destination as "ORIGIN / DESTINATION"
        let portDisplay = '';
        if (mappedOrigin && mappedDest) {
            portDisplay = `${mappedOrigin} / ${mappedDest}`;
        } else if (mappedOrigin) {
            portDisplay = mappedOrigin;
        }

        setFormData(prev => ({
            ...prev,
            pax_name: prev.pax_name || scanData.fullName,
            flight_no: scanData.flightNo,
            port_of_dep_dest: portDisplay || prev.port_of_dep_dest,
            flight_date: scanData.flightDate ? scanData.flightDate : prev.flight_date
        }));
    }
  }, []);

  return (
    <div className="space-y-4 w-full pb-20">
      {/* Header Panel */}
      <div className="flex justify-between items-center bg-white px-4 py-3 border-b border-slate-200 rounded-xl border">
        <div className="flex items-center space-x-4">
          <button onClick={() => navigate(goBackPath)} className="p-2 bg-slate-50 border border-slate-200 rounded-md hover:bg-slate-100 transition-colors">
            <ArrowLeft size={20} className="text-slate-600" />
          </button>
          <div>
            <h1 className="text-2xl font-bold text-slate-800 tracking-tight flex items-center">
              {isViewOnly ? `View O.S. No: ${osNo}/${osYear}` : isEditing ? `Modify O.S. No: ${osNo}/${osYear}` : 'Register New O.S. Case'}
              <span className="ml-3 text-sm font-medium px-2 py-0.5 bg-brand-100 text-brand-700 rounded border border-brand-200">
                  {formData.os_date}
              </span>
            </h1>
          </div>
        </div>
        
        <div className="flex items-center space-x-3">
            {isViewOnly ? (
                <span className="bg-blue-100 text-blue-800 border border-blue-300 px-3 py-1.5 rounded-lg text-sm font-bold flex items-center shadow-sm">
                    <FileText className="mr-1.5" size={16} /> VIEW ONLY — ADJUDICATED
                </span>
            ) : formData.is_draft === 'N' ? (
                <span className="bg-emerald-100 text-emerald-800 border border-emerald-300 px-3 py-1.5 rounded-lg text-sm font-bold flex items-center">
                    <CheckCircle className="mr-1.5" size={16} /> Submitted
                </span>
            ) : (
                <span className="bg-amber-100 text-amber-800 border border-amber-300 px-3 py-1.5 rounded-lg text-sm font-bold flex items-center">
                    <AlertCircle className="mr-1.5" size={16} /> Draft
                </span>
            )}
        </div>
      </div>

      {errorMsg && (
        <div className="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded-lg flex flex-col items-start">
            <div className="flex items-start">
                <AlertCircle className="shrink-0 mr-3 mt-0.5" size={20} />
                <div>
                    <h4 className="font-bold text-sm">Validation Error</h4>
                    <p className="text-sm">{errorMsg}</p>
                </div>
            </div>
            {(Object.keys(fieldErrors).length > 0 || Object.keys(itemErrors).length > 0) && (
                <div className="ml-8 mt-2 w-full pt-2 border-t border-red-200/60 max-h-40 overflow-y-auto pr-2 custom-scrollbar">
                    <p className="text-xs font-bold mb-1 uppercase tracking-wider text-red-800/80">Missing / Invalid Fields:</p>
                    <ul className="list-disc pl-4 text-xs space-y-1 font-medium text-red-800">
                        {Object.entries(fieldErrors).map(([key, msg]) => (
                            <li key={key}>{msg}</li>
                        ))}
                        {Object.entries(itemErrors).flatMap(([idx, errs]) => 
                            Object.entries(errs).map(([key, msg]) => (
                                <li key={`item-${idx}-${key}`}>Seized Item {Number(idx) + 1}: {msg}</li>
                            ))
                        )}
                    </ul>
                </div>
            )}
        </div>
      )}

      {showPassportAlert && (
          <div className="bg-indigo-50 border border-indigo-200 text-indigo-700 p-4 rounded-lg flex items-center justify-between">
              <div className="flex items-center">
                  <AlertCircle className="mr-3" size={24} />
                  <div>
                      <h4 className="font-bold">Previous Passport Records Found!</h4>
                      <p className="text-sm">Found existing passport numbers in database for <b>{formData.pax_name}</b> (DOB: {formData.pax_date_of_birth}): <b>{oldPassportsSuggestion.join(', ')}</b>.</p>
                  </div>
              </div>
              <div className="flex space-x-2">
                  <button onClick={() => setShowPassportAlert(false)} className="px-3 py-1.5 text-slate-600 hover:bg-indigo-100 rounded font-medium text-sm">Discard</button>
                  <button onClick={acceptPassportSuggestion} className="px-3 py-1.5 bg-indigo-600 text-white hover:bg-indigo-700 rounded font-medium text-sm shadow-sm">Keep Records</button>
              </div>
          </div>
      )}

      <div className="space-y-6 mt-2">
        <fieldset disabled={isViewOnly} className="space-y-6">
        {/* Top Details Grid */}
        <div className="grid grid-cols-1 xl:grid-cols-5 gap-6">
            <div className="bg-white p-5 rounded-xl border border-slate-200 xl:col-span-2 flex flex-col h-full">
                <h2 className="text-sm font-bold text-slate-800 uppercase tracking-wider mb-4 border-b border-slate-100 pb-2 flex items-center shrink-0">
                    <FileDigit className="mr-2 text-brand-500" size={16} /> Case Registration Details
                </h2>
                <div className="flex-1 flex flex-col space-y-3">
                    <div className="grid grid-cols-2 gap-3">
                        <div>
                            <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">O.S. No.</label>
                            <input
                                type="text"
                                inputMode="numeric"
                                id="field-os_no"
                                className={numericInputClass(!!fieldErrors.os_no, "w-full px-3 py-2 bg-slate-50 border border-slate-300 rounded focus:ring-2 focus:ring-brand-500 text-sm")}
                                value={formData.os_no}
                                onChange={async e => {
                                  const raw = e.target.value;
                                  const sanitized = sanitizeInteger(raw);
                                  setFormData(prev => ({ ...prev, os_no: sanitized }));

                                  if (!sanitized) {
                                    setFieldError('os_no', 'O.S. No. is required.');
                                    return;
                                  }
                                  if (sanitized !== raw) {
                                    setFieldError('os_no', 'Digits only.');
                                    return;
                                  }
                                  clearFieldError('os_no');

                                  // Debounced uniqueness check — waits 500ms after user stops typing
                                  if (!isEditing) {
                                    clearTimeout(osNoCheckTimer.current);
                                    osNoCheckTimer.current = setTimeout(async () => {
                                      try {
                                        const yr = formData.os_date ? new Date(formData.os_date).getFullYear() : new Date().getFullYear();
                                        const { data: result } = await api.get('/os/check-os-no', { params: { os_no: sanitized, os_year: yr } });
                                        if (result.exists) setFieldError('os_no', `O.S. No. ${sanitized}/${yr} already exists!`);
                                      } catch { /* ignore network errors */ }
                                    }, 500);
                                  }
                                }}
                            />
                            {fieldErrors.os_no && (
                              <p className="mt-1 text-xs font-semibold text-red-600">{fieldErrors.os_no}</p>
                            )}
                        </div>
                        <div>
                            <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">O.S. Date</label>
                            <DatePicker
                                id="field-os_date"
                                value={formData.os_date}
                                onChange={isoDate => setFormData({ ...formData, os_date: isoDate })}
                                inputClassName="w-full px-3 py-2 bg-slate-50 border border-slate-300 focus:ring-brand-500 rounded focus:ring-2 text-sm"
                                error={!!fieldErrors.os_date}
                            />
                        </div>
                    </div>
                    <div className="grid grid-cols-2 gap-3">
                        <div>
                            <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">Shift</label>
                            <select id="field-shift" className={`w-full px-3 py-2 bg-slate-50 border ${fieldErrors.shift ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-brand-500'} rounded focus:ring-2 text-sm`} value={formData.shift} onChange={e => setFormData({...formData, shift: e.target.value})}>
                                <option>Day</option><option>Night</option>
                            </select>
                        </div>
                        <div>
                            <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">Booked By</label>
                            <select id="field-booked_by" className={`w-full px-3 py-2 bg-slate-50 border ${fieldErrors.booked_by ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-brand-500'} rounded focus:ring-2 text-sm`} value={formData.booked_by} onChange={e => setFormData({...formData, booked_by: e.target.value})}>
                                <option>Batch A</option><option>Batch B</option><option>Batch C</option><option>Batch D</option>
                                <option>AIU A</option><option>AIU B</option><option>AIU C</option><option>AIU D</option>
                            </select>
                        </div>
                    </div>
                    <div className="grid grid-cols-2 gap-x-3 gap-y-1">
                        <div>
                            <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">Case Type</label>
                            <select id="field-case_type" className={`w-full px-3 py-2 bg-slate-50 border ${fieldErrors.case_type ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-brand-500'} rounded focus:ring-2 text-sm font-medium text-slate-800`} value={formData.case_type} onChange={e => setFormData({...formData, case_type: e.target.value})}>
                                <option>Non-Bonafide</option>
                                <option>Mis-Declaration</option>
                                <option>Concealment</option>
                                <option>Trade Goods</option>
                                <option>Unclaimed</option>
                                <option>Export Case</option>
                            </select>
                        </div>
                        <div>
                            <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">Detention / Seizure Date</label>
                        <DatePicker
                            id="field-detention_date"
                            value={formData.detention_date}
                            onChange={isoDate => setFormData({ ...formData, detention_date: isoDate })}
                            inputClassName="w-full px-3 py-2 bg-slate-50 border border-slate-300 focus:ring-brand-500 rounded focus:ring-2 text-sm"
                            error={!!fieldErrors.detention_date}
                        />
                    </div>
                </div>
            </div>
        </div>

        {/* Passenger Information Panel */}
            <div className="bg-white p-5 rounded-xl border border-slate-200 xl:col-span-3 relative overflow-hidden flex flex-col h-full">
                <div className="absolute top-0 left-0 w-1 h-full bg-blue-500"></div>
                <div className="flex justify-between items-center mb-4 border-b border-slate-100 pb-2">
                    <h2 className="text-sm font-bold text-slate-800 uppercase tracking-wider flex items-center">
                        <User className="mr-2 text-blue-500" size={16} /> Passenger & Passport Information
                    </h2>
                    <PassportScanner onScan={handleScan} />
                </div>
                
                <div className="grid grid-cols-5 gap-4">
                    <div className="col-span-2">
                        <label className="block text-xs font-semibold text-slate-500 uppercase tracking-wider mb-1">Passenger Name</label>
                        <input id="field-pax_name" type="text" className={`w-full px-3 py-1.5 border ${fieldErrors.pax_name ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-blue-500'} rounded text-sm focus:ring-2 uppercase font-medium`} value={formData.pax_name} onChange={e => setFormData({...formData, pax_name: e.target.value.toUpperCase()})} />
                    </div>
                    <div className="col-span-1">
                        <label className="block text-xs font-semibold text-slate-500 uppercase tracking-wider mb-1">Date of Birth</label>
                        <DatePicker
                            id="field-pax_date_of_birth"
                            value={formData.pax_date_of_birth}
                            onChange={isoDate => setFormData({ ...formData, pax_date_of_birth: isoDate })}
                            inputClassName="w-full px-3 py-1.5 border border-slate-300 focus:ring-blue-500 rounded text-sm focus:ring-2 font-medium"
                            error={!!fieldErrors.pax_date_of_birth}
                        />
                    </div>
                    <div className="col-span-2">
                        <label className="block text-xs font-semibold text-slate-500 uppercase tracking-wider mb-1">Father's/Husband's Name</label>
                        <input type="text" className="w-full px-3 py-1.5 border border-slate-300 rounded text-sm focus:ring-2 focus:ring-blue-500 uppercase font-medium" value={formData.father_name} onChange={e => setFormData({...formData, father_name: e.target.value.toUpperCase()})} />
                    </div>

                    <div className="col-span-1">
                        <label className="block text-xs font-semibold text-slate-500 uppercase tracking-wider mb-1">
                            Nationality
                        </label>
                        <input
                            id="field-pax_nationality"
                            type="text"
                            list="nationality-list-opts"
                            className={`w-full px-3 py-1.5 border ${fieldErrors.pax_nationality ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-blue-500'} rounded text-sm focus:ring-2 uppercase font-medium`}
                            value={formData.pax_nationality}
                            onChange={e => setFormData({...formData, pax_nationality: e.target.value.toUpperCase()})}
                            placeholder="Type or select nationality"
                            autoComplete="off"
                        />
                        <datalist id="nationality-list-opts">
                            {NATIONALITY_LIST.map(n => <option key={n} value={n} />)}
                        </datalist>
                    </div>
                    <div className="col-span-1">
                        <label className="block text-xs font-semibold text-slate-500 uppercase tracking-wider mb-1">Normal Residence At</label>
                        <select id="field-residence_at" className={`w-full px-3 py-1.5 border ${fieldErrors.residence_at ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-blue-500'} rounded text-sm focus:ring-2 uppercase`} value={formData.residence_at} onChange={e => setFormData({...formData, residence_at: e.target.value.toUpperCase()})}>
                            <option value="INDIA">INDIA</option>
                            <option value="ABROAD">ABROAD</option>
                        </select>
                    </div>
                    
                    <div className="col-span-3 relative group">
                        <label className="block text-xs font-semibold text-slate-500 uppercase tracking-wider mb-1 truncate">
                            Place Of Issue Of PP
                        </label>
                        <input id="field-pp_issue_place" type="text" className={`w-full px-3 py-1.5 border ${fieldErrors.pp_issue_place ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-blue-500'} rounded text-sm focus:ring-2 uppercase`} value={formData.pp_issue_place} onChange={e => setFormData({...formData, pp_issue_place: e.target.value.toUpperCase()})} />
                    </div>
                    
                    <div className="col-span-5">
                        <label className="block text-xs font-semibold text-slate-500 uppercase tracking-wider mb-1">Passenger Address</label>
                        <input id="field-pax_address1" type="text" className={`w-full px-3 py-1.5 border ${fieldErrors.pax_address1 ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-blue-500'} rounded text-sm focus:ring-2 uppercase mb-1`} placeholder="Enter Full Address" value={formData.pax_address1} onChange={e => setFormData({...formData, pax_address1: e.target.value.toUpperCase()})} />
                    </div>

                    <div className="col-span-1">
                        <label className="block text-xs font-semibold text-slate-500 uppercase tracking-wider mb-1">Passport No.</label>
                        <input id="field-passport_no" type="text" className={`w-full px-3 py-1.5 border ${fieldErrors.passport_no ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-blue-500'} rounded text-sm focus:ring-2 uppercase font-bold text-slate-800`} value={formData.passport_no} onChange={e => setFormData({...formData, passport_no: e.target.value.toUpperCase()})} />
                    </div>
                    <div className="col-span-1">
                        <label className="block text-xs font-semibold text-slate-500 uppercase tracking-wider mb-1">Passport Date (Expiry)</label>
                        <DatePicker
                            id="field-passport_date"
                            value={formData.passport_date}
                            onChange={isoDate => setFormData({ ...formData, passport_date: isoDate })}
                            inputClassName="w-full px-3 py-1.5 border border-slate-300 focus:ring-blue-500 rounded text-sm focus:ring-2"
                            error={!!fieldErrors.passport_date}
                        />
                    </div>

                    <div className="col-span-3">
                        <label className="block text-xs font-semibold text-slate-500 uppercase tracking-wider mb-1 flex items-center justify-between">
                            Old P.P. Nos. <span className="text-[10px] lowercase normal-case font-normal text-slate-400 -mt-0.5">(Separate with ;)</span>
                        </label>
                        <input type="text" className="w-full px-3 py-1.5 border border-slate-300 rounded text-sm focus:ring-2 focus:ring-blue-500 uppercase" placeholder="Leave blank if not avail" value={formData.old_passport_no} onChange={e => setFormData({...formData, old_passport_no: e.target.value.toUpperCase()})} />
                        {oldPassportsSuggestion.length > 0 && (
                          <div className="flex flex-wrap gap-1.5 mt-1.5">
                            <span className="text-[10px] text-indigo-600 font-semibold self-center">Found in COPS:</span>
                            {oldPassportsSuggestion.map(pp => (
                              <button
                                key={pp}
                                type="button"
                                onClick={() => {
                                  setFormData(prev => ({
                                    ...prev,
                                    old_passport_no: prev.old_passport_no ? `${prev.old_passport_no}; ${pp}` : pp,
                                  }));
                                  setOldPassportsSuggestion(prev => prev.filter(p => p !== pp));
                                }}
                                className="text-[11px] bg-indigo-50 border border-indigo-300 text-indigo-700 px-2 py-0.5 rounded-full font-semibold hover:bg-indigo-100 transition-colors"
                              >
                                + {pp}
                              </button>
                            ))}
                          </div>
                        )}
                    </div>
                </div>
            </div>
        </div>

        {/* Flight Panel */}
        <div className="bg-white p-5 rounded-xl border border-slate-200 relative overflow-hidden">
            <div className="absolute top-0 left-0 w-1 h-full bg-emerald-500"></div>
            <div className="flex items-center justify-between mb-4 border-b border-slate-100 pb-2">
                <h2 className="text-sm font-bold text-slate-800 uppercase tracking-wider flex items-center">
                    <Plane className="mr-2 text-emerald-500" size={16} /> Interception & Travel Details
                </h2>
                {/* Reuse scanner here so officers can scan boarding passes directly in the flight section */}
                <PassportScanner onScan={handleScan} />
            </div>
            <div className="grid grid-cols-4 gap-4">
                <div>
                    <label className="block text-xs font-semibold text-slate-500 uppercase tracking-wider mb-1">Flight Number</label>
                    <input id="field-flight_no" type="text" className={`w-full px-3 py-1.5 bg-slate-50 border ${fieldErrors.flight_no ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-emerald-500'} rounded text-sm font-medium text-slate-800 focus:ring-2 uppercase`} value={formData.flight_no} onChange={e => setFormData({...formData, flight_no: e.target.value.toUpperCase()})} />
                </div>
                <div>
                    <label className="block text-xs font-semibold text-slate-500 uppercase tracking-wider mb-1">Flight Date</label>
                    <DatePicker
                        id="field-flight_date"
                        value={formData.flight_date}
                        onChange={isoDate => setFormData({ ...formData, flight_date: isoDate })}
                        inputClassName="w-full px-3 py-1.5 bg-slate-50 border border-slate-300 focus:ring-emerald-500 rounded text-sm font-medium text-slate-800 focus:ring-2"
                        error={!!fieldErrors.flight_date}
                    />
                </div>
                <div>
                    <label className="block text-xs font-semibold text-slate-500 uppercase tracking-wider mb-1">Port of Dep/Dest</label>
                    <input id="field-port_of_dep_dest" type="text" className={`w-full px-3 py-1.5 bg-slate-50 border ${fieldErrors.port_of_dep_dest ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-emerald-500'} rounded text-sm focus:ring-2 uppercase text-slate-800`} value={formData.port_of_dep_dest} onChange={e => setFormData({...formData, port_of_dep_dest: e.target.value.toUpperCase()})} />
                </div>
                <div>
                    <label className="block text-xs font-semibold text-slate-500 uppercase tracking-wider mb-1">
                      {formData.case_type === 'Export Case' ? 'Supposed Destination' : 'Arrived From'}
                    </label>
                    {formData.case_type === 'Export Case' ? (
                      <input
                        id="field-arrived_from"
                        type="text"
                        className={`w-full px-3 py-1.5 bg-slate-50 border ${fieldErrors.arrived_from ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-emerald-500'} rounded text-sm focus:ring-2 uppercase`}
                        value={formData.arrived_from === 'Others' ? '' : formData.arrived_from}
                        placeholder="e.g. DUBAI, SINGAPORE"
                        onChange={e => setFormData({...formData, arrived_from: e.target.value.toUpperCase() || 'Others'})}
                      />
                    ) : (
                      <select id="field-arrived_from" className={`w-full px-3 py-1.5 bg-slate-50 border ${fieldErrors.arrived_from ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-emerald-500'} rounded text-sm focus:ring-2`} value={formData.arrived_from} onChange={e => setFormData({...formData, arrived_from: e.target.value})}>
                          <option>Nepal</option><option>Bhutan</option><option>Myanmar</option><option>Others</option>
                      </select>
                    )}
                </div>

                <div>
                    <label className="block text-[10px] font-semibold text-slate-500 uppercase tracking-wider mb-1 truncate" title={formData.case_type === 'Export Case' ? 'Proposed Date of Travel / Departure' : 'Date of Departure From INDIA (For Foreign Nationals, if DOD from India is Not Available, Type N.A.)'}>
                      {formData.case_type === 'Export Case' ? 'Proposed Date of Travel' : 'Date of Departure from India (Type N.A. If none)'}
                    </label>
                    <DatePicker
                        id="field-date_of_departure"
                        value={formData.date_of_departure}
                        onChange={val => setFormData({ ...formData, date_of_departure: val })}
                        inputClassName="w-full px-3 py-1.5 bg-slate-50 border border-slate-300 focus:ring-emerald-500 rounded text-sm focus:ring-2 uppercase"
                        error={!!fieldErrors.date_of_departure}
                        placeholder="dd/mm/yyyy or N.A."
                        allowNA
                    />
                </div>
                <div>
                    <label className="block text-[10px] font-semibold text-slate-500 uppercase tracking-wider mb-1 truncate">
                      {formData.case_type === 'Export Case' ? 'Stay Abroad (N/A for Export)' : 'Stay Abroad in days'}
                    </label>
                    {formData.case_type === 'Export Case' ? (
                      <input type="text" readOnly className="w-full px-3 py-1.5 bg-slate-200 border border-slate-300 rounded text-sm text-slate-500 cursor-not-allowed" value="N/A" />
                    ) : (
                      <input type="text" readOnly className="w-full px-3 py-1.5 bg-amber-50 border border-amber-300 rounded text-sm text-amber-900 font-bold cursor-not-allowed" value={formData.stay_abroad_days} />
                    )}
                </div>
                <div>
                    <label className="block text-[10px] font-semibold text-slate-500 uppercase tracking-wider mb-1">Previous Visits</label>
                    <input id="field-previous_visits" type="text" className={`w-full px-3 py-1.5 bg-slate-50 border ${fieldErrors.previous_visits ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-emerald-500'} rounded text-sm focus:ring-2 uppercase`} value={formData.previous_visits} onChange={e => setFormData({...formData, previous_visits: e.target.value.toUpperCase()})} />
                </div>

            </div>
        </div>

        {/* Goods / Items Grid */}
        <div className="bg-white rounded-xl border border-slate-200 overflow-hidden flex flex-col">
            <div className="p-4 border-b border-slate-200 bg-slate-50 flex justify-between items-center">
                <h2 className="text-sm font-bold text-slate-800 uppercase flex items-center tracking-wider">
                    <FileText className="mr-2 text-orange-500" size={16} /> Seized Goods Registration
                </h2>
                <button 
                   onClick={(e) => { e.preventDefault(); setItems([...items, { items_desc: '', items_qty: 1, items_uqc: 'NOS', value_per_piece: 0, items_value: 0, items_fa: 0, items_fa_type: 'value', items_fa_qty: 0, items_fa_uqc: 'NOS', cumulative_duty_rate: 35, items_duty: 0, items_release_category: '', items_duty_type: 'Miscellaneous-22'}]); }}
                   className="text-xs px-3 py-1.5 bg-white text-orange-700 hover:bg-orange-50 border border-orange-200 rounded font-bold flex items-center transition-colors uppercase tracking-wider">
                    <Plus size={14} className="mr-1" /> Add Item
                </button>
            </div>
            
            <div className="overflow-auto">
                <table className="w-full text-sm text-left whitespace-nowrap">
                    <thead className="text-[10px] text-slate-500 uppercase bg-slate-100 border-b border-slate-200 tracking-wider">
                        <tr>
                            <th className="px-3 py-2 font-bold text-center w-10">S.No</th>
                            <th className="px-3 py-2 font-bold w-48">Description of Goods</th>
                            <th className="px-3 py-2 font-bold w-28 text-center" title="Category: Under OS = seized (liable to action) | Under Duty = allowed on duty payment">
                                Category
                            </th>
                            <th className="px-3 py-2 font-bold w-24 text-center" title="Disposal Action (RF/REF/CONFS)">Disposal Action</th>
                            <th className="px-3 py-2 font-bold w-32">Offence / Duty Type</th>
                            <th className="px-3 py-2 font-bold w-36 text-center">Quantity &amp; Unit</th>
                            <th className="px-3 py-2 font-bold w-24 text-right">Rate / Piece (₹)</th>
                            <th className="px-3 py-2 font-bold w-28 text-center" title="Free Allowance — Value (₹) deducted before duty, or Qty allowed free (for seized goods)">Free Allowance</th>
                            <th className="px-3 py-2 font-bold w-28 text-right bg-amber-50">Value after FA (₹)</th>
                            <th className="px-3 py-2 font-bold w-20 text-center" title="Cumulative Duty Rate (%) applicable on dutiable value">Duty Rate (%)</th>
                            <th className="px-3 py-2 font-bold w-28 text-right bg-brand-50">Total Duty (₹)</th>
                            <th className="px-3 py-2 font-bold w-12"></th>
                        </tr>
                    </thead>
                    <tbody className="divide-y divide-slate-100 bg-white">
                        {items.length === 0 ? (
                            <tr><td colSpan={12} className="text-center py-8 text-slate-400">Click "Add Item" to add seized goods data.</td></tr>
                        ) : (
                            items.map((itm, idx) => (
                                <ItemRow
                                    key={idx}
                                    itm={itm}
                                    idx={idx}
                                    rowErrors={itemErrors[idx]}
                                    updateItem={updateItem}
                                    onRemove={onRemove}
                                    onSetFieldError={setItemFieldError}
                                    onClearFieldError={clearItemFieldError}
                                    onDescBlur={onDescBlur}
                                    descDatalistId="item-desc-datalist"
                                />
                            ))
                        )}
                    </tbody>
                    <tfoot className="bg-slate-100 border-t border-slate-200">
                        <tr>
                            <td colSpan={8} className="px-4 py-2 text-right text-xs font-bold text-slate-600 uppercase tracking-wider">Overall Value (after FA):</td>
                            <td className="px-4 py-2 text-right text-lg font-black text-amber-800">
                                ₹ {totalValueAfterFA.toLocaleString('en-IN', { maximumFractionDigits: 0 })}
                            </td>
                            <td colSpan={3}></td>
                        </tr>
                        <tr className="border-t border-slate-200">
                            <td colSpan={10} className="px-4 py-2 text-right text-xs font-bold text-slate-600 uppercase tracking-wider">Overall Duty:</td>
                            <td className="px-4 py-2 text-right text-lg font-black text-brand-800">
                                ₹ {totalDuty.toLocaleString('en-IN', { maximumFractionDigits: 0 })}
                            </td>
                            <td></td>
                        </tr>
                    </tfoot>
                </table>
            </div>
        </div>

        {/* Item-description autocomplete datalist — memoized: only re-renders when DB data loads, not on every formData keystroke */}
        {descDatalist}

        {/* Superintendent Remarks */}
        <div className="bg-white rounded-xl border border-slate-200 overflow-hidden flex flex-col mt-6 shadow-sm">
            <div className="p-4 border-b border-slate-200 bg-slate-50 flex justify-between items-center">
                <h2 className="text-sm font-bold text-slate-800 uppercase flex items-center tracking-wider">
                    <FileText className="mr-2 text-indigo-500" size={16} /> Superintendent's Remarks & Findings
                </h2>
                <button
                  type="button"
                  onClick={(e) => { e.preventDefault(); triggerGenerateRemarks('SUPDT'); }}
                  disabled={remarksLoading}
                  title={remarksLoading ? 'Loading legal statutes…' : 'Auto-generate remarks from seized items'}
                  className="text-[11px] px-3 py-1.5 bg-indigo-50 text-indigo-700 hover:bg-indigo-100 border border-indigo-200 rounded font-bold flex items-center transition-colors shadow-sm disabled:opacity-60 uppercase tracking-wider"
                >
                  <Wand2 size={13} className="mr-1.5" />
                  {remarksLoading ? 'Loading Statutes…' : 'Auto-Generate From Items ✨'}
                </button>
            </div>
            <div className="p-4 bg-white">
                <div className="flex justify-between items-center mb-1">
                    <label className="block text-xs font-semibold text-slate-500 uppercase tracking-wider">Case Remarks</label>
                    <span className={`text-xs font-semibold ${supdtsRemarks.length > 1500 ? 'text-red-600' : supdtsRemarks.length > 1275 ? 'text-orange-500' : 'text-slate-400'}`}>
                        {supdtsRemarks.length} / 1500
                    </span>
                </div>
                <textarea
                    id="field-supdts_remarks"
                    rows={6}
                    className={`w-full px-3 py-2 bg-slate-50 border rounded text-sm focus:ring-2 focus:ring-emerald-500 resize-none ${supdtsRemarks.length > 1500 || fieldErrors.supdts_remarks ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300'}`}
                    value={supdtsRemarks}
                    onChange={e => setSupdtsRemarks(e.target.value.slice(0, 1500))}
                    placeholder="Click the Auto-Generate button to construct remarks based on items, or type manually..."
                />
                {fieldErrors.supdts_remarks && <p className="mt-1 text-xs font-semibold text-red-600">{fieldErrors.supdts_remarks}</p>}
            </div>
        </div>
        </fieldset>

      {/* Warning Note about Default Values */}
      {!isViewOnly && (
        <div className="bg-amber-50 border-l-4 border-amber-500 p-4 mt-6 rounded-r-lg">
        <div className="flex items-start">
          <div className="flex-shrink-0 mt-0.5">
            <AlertCircle className="h-5 w-5 text-amber-600" aria-hidden="true" />
          </div>
          <div className="ml-3">
            <h3 className="text-sm font-bold text-amber-800 uppercase tracking-wider">
              {isEditing ? 'Please Review Your Selections' : 'Please Review Default Selections'}
            </h3>
            <div className="mt-2 text-sm text-amber-700">
              <p>
                {isEditing 
                  ? 'Please review the following key selections and change them if necessary before submitting the O.S. for adjudication:'
                  : 'The system has pre-selected some data by default. Please verify and change them if necessary before submitting the O.S. for adjudication:'}
              </p>
              <ul className="list-disc pl-5 mt-1 space-y-1 font-medium">
                <li><b>Shift:</b> {formData.shift}</li>
                <li><b>Booked By:</b> {formData.booked_by}</li>
                <li><b>Case Type:</b> {formData.case_type}</li>
                <li><b>Nationality:</b> {formData.pax_nationality}</li>
                <li><b>Normal Residence At:</b> {formData.residence_at || 'INDIA'}</li>
                <li><b>{formData.case_type === 'Export Case' ? 'Supposed Destination' : 'Arrived From'}:</b> {formData.arrived_from}</li>
              </ul>
            </div>
          </div>
        </div>
      </div>
      )}

      {/* Form Action Buttons at Bottom */}
      {!isViewOnly && (
      <div className="flex space-x-4 justify-end bg-white p-4 rounded-xl border border-slate-200 mt-4 mb-8">
        <button
          onClick={() => setShowExitModal(true)}
          className="px-6 py-2.5 bg-white border border-slate-300 text-slate-700 rounded-lg hover:bg-slate-50 font-medium transition-colors"
        >
           Exit Without Saving
        </button>
        {formData.is_draft !== 'N' && (
            <button
                onClick={() => submitData('Y')}
                disabled={isSubmitting}
                className="px-6 py-2.5 bg-amber-500 border border-amber-600 text-white rounded-lg hover:bg-amber-600 font-medium transition-colors"
            >
                Save as Draft
            </button>
        )}
        <button
            onClick={() => submitData('N')}
            disabled={isSubmitting}
            className="px-6 py-2.5 bg-brand-600 text-white rounded-lg hover:bg-brand-700 font-bold transition-colors flex items-center disabled:opacity-50"
        >
          <Save size={18} className="mr-2" />
          {formData.is_draft === 'N' ? 'Update O.S. Details' : 'Submit O.S. For Adjudication'}
        </button>
      </div>
      )}

      </div>

      {/* Contextual questions modal — appears before auto-generating remarks */}
      {showContextModal && (
        <div className="fixed inset-0 bg-slate-900/50 flex items-center justify-center z-50">
          <div className="bg-white rounded-xl shadow-2xl border border-slate-200 w-full max-w-lg p-6 space-y-5">
            <div>
              <h3 className="text-base font-bold text-slate-800">Additional Information Required</h3>
              <p className="text-xs text-slate-500 mt-1">
                Please answer the following questions to generate accurate remarks.
              </p>
            </div>
            <div className="space-y-4">
              {contextQuestions.map(q => (
                <div key={q.key} className="border border-slate-200 rounded-lg p-4 bg-slate-50">
                  <p className="text-sm font-medium text-slate-700 mb-3">{q.question}</p>
                  <div className="flex gap-3">
                    <button
                      type="button"
                      onClick={() => setContextAnswers(prev => ({ ...prev, [q.key]: true }))}
                      className={`flex-1 py-2 px-3 rounded-lg border text-sm font-semibold transition-colors ${contextAnswers[q.key] === true ? 'bg-green-600 text-white border-green-600' : 'bg-white text-slate-700 border-slate-300 hover:border-green-400'}`}
                    >
                      {q.yesLabel || 'Yes'}
                    </button>
                    <button
                      type="button"
                      onClick={() => setContextAnswers(prev => ({ ...prev, [q.key]: false }))}
                      className={`flex-1 py-2 px-3 rounded-lg border text-sm font-semibold transition-colors ${contextAnswers[q.key] === false ? 'bg-red-600 text-white border-red-600' : 'bg-white text-slate-700 border-slate-300 hover:border-red-400'}`}
                    >
                      {q.noLabel || 'No'}
                    </button>
                  </div>
                </div>
              ))}
            </div>
            <div className="flex gap-3 pt-1">
              <button
                type="button"
                onClick={() => setShowContextModal(false)}
                className="flex-1 py-2 px-4 rounded-lg border border-slate-300 text-slate-700 text-sm font-medium hover:bg-slate-50"
              >
                Cancel
              </button>
              <button
                type="button"
                disabled={contextQuestions.some(q => contextAnswers[q.key] === undefined)}
                onClick={handleContextSubmit}
                className="flex-1 py-2 px-4 rounded-lg bg-indigo-600 text-white text-sm font-bold hover:bg-indigo-700 disabled:opacity-50 flex items-center justify-center gap-1.5"
              >
                <Wand2 size={14} /> Generate Remarks
              </button>
            </div>
          </div>
        </div>
      )}

      {showExitModal && (
        <div className="fixed inset-0 bg-slate-900/40 flex items-center justify-center z-50">
          <div className="relative bg-white rounded-xl shadow-xl border border-slate-200 w-full max-w-md p-6 space-y-4">
            <h3 className="text-lg font-semibold text-slate-800">Exit O.S. Case</h3>
            <p className="text-sm text-slate-600">
              Choose how you want to exit this O.S. form.
            </p>
            <div className="space-y-2">
              <button
                className="w-full px-4 py-2 bg-brand-600 text-white rounded-lg hover:bg-brand-700 text-sm font-medium shadow-sm disabled:opacity-60"
                disabled={isSubmitting}
                onClick={async () => {
                  await submitData('N');
                  setShowExitModal(false);
                }}
              >
                Save and Exit
              </button>
              <button
                className="w-full px-4 py-2 bg-amber-500 text-white rounded-lg hover:bg-amber-600 text-sm font-medium shadow-sm disabled:opacity-60"
                disabled={isSubmitting}
                onClick={async () => {
                  await submitData('Y');
                  setShowExitModal(false);
                }}
              >
                Save as Draft and Exit
              </button>
              <button
                className="w-full px-4 py-2 bg-white border border-slate-300 text-slate-700 rounded-lg hover:bg-slate-50 text-sm font-medium"
                onClick={() => {
                  setShowExitModal(false);
                  navigate(goBackPath);
                }}
              >
                Exit without Saving
              </button>
            </div>
            <button
              className="absolute top-3 right-3 text-slate-400 hover:text-slate-600 text-sm"
              onClick={() => setShowExitModal(false)}
            >
              ×
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
