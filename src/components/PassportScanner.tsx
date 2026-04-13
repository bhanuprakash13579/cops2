/* eslint-disable @typescript-eslint/no-explicit-any, @typescript-eslint/no-unused-vars */
import { useState, useEffect, useRef } from 'react';
import { Scan, X, AlertCircle } from 'lucide-react';

interface PassportScannerProps {
    onScan: (scannedData: any) => void;
}

export default function PassportScanner({ onScan }: PassportScannerProps) {
    const [isOpen, setIsOpen] = useState(false);
    const [scanBuffer, setScanBuffer] = useState('');
    const [error, setError] = useState('');
    const inputRef = useRef<HTMLInputElement>(null);

    // Auto-focus the hidden input when scanner modal is opened
    useEffect(() => {
        if (isOpen && inputRef.current) {
            inputRef.current.focus();
            // eslint-disable-next-line react-hooks/set-state-in-effect
            setScanBuffer('');
            setError('');
        }
    }, [isOpen]);

    const processScanData = (scannedString: string) => {
        const cleanString = scannedString.replace(/\s+/g, '');
        const upper = scannedString.toUpperCase();
        
        try {
            // ---- PASSPORT MRZ CHECK ----  
            if (cleanString.startsWith('P') && cleanString.length >= 88) {
                const line1 = cleanString.substring(0, 44);
                const line2 = cleanString.substring(44, 88);

                const nationality = line1.substring(2, 5).replace(/</g, '');
                const nameParts = line1.substring(5).split('<<');
                const lastName = nameParts[0].replace(/</g, ' ');
                const firstName = nameParts.length > 1 ? nameParts[1].replace(/</g, ' ') : '';
                const fullName = `${firstName} ${lastName}`.trim().toUpperCase();

                const passportNo = line2.substring(0, 9).replace(/</g, '');

                const dobStr = line2.substring(13, 19);
                const yearPrefix = parseInt(dobStr.substring(0, 2)) > 50 ? '19' : '20';
                const dob = `${yearPrefix}${dobStr.substring(0,2)}-${dobStr.substring(2,4)}-${dobStr.substring(4,6)}`;
                
                const gender = line2.substring(20, 21); // 'M', 'F', or '<'

                const expStr = line2.substring(21, 27);
                const expYearPrefix = parseInt(expStr.substring(0, 2)) > 50 ? '19' : '20';
                const expiryDate = `${expYearPrefix}${expStr.substring(0,2)}-${expStr.substring(2,4)}-${expStr.substring(4,6)}`;

                setIsOpen(false);
                onScan({
                    type: 'PASSPORT',
                    passportNo,
                    fullName,
                    nationality,
                    dateOfBirth: dob,
                    expiryDate: expiryDate,
                    gender,
                });
                return;
            }

            // ---- BOARDING PASS BCBP CHECK ----
            if (upper.startsWith('M1')) {
                 // Format: M1DOE/JOHN MR         EABCDEF DELBOMAI 0123 125Y012A0015 100
                 const passengerName = upper.substring(2, 22).trim();
                 const pnr = upper.substring(22, 29).trim();
                 const origin = upper.substring(30, 33);
                 const destination = upper.substring(33, 36);
                 const airlineCode = upper.substring(36, 39).trim();
                 const flightNoStr = upper.substring(39, 44).trim();
                 
                 // Flight No: combine airline code and numbers
                 const fullFlightNo = `${airlineCode}-${flightNoStr}`;

                 // Julian Date to YYYY-MM-DD (approx based on current year)
                 const julianDateStr = upper.substring(44, 47);
                 let flightDate = '';
                 if (julianDateStr && !isNaN(parseInt(julianDateStr))) {
                     const date = new Date(new Date().getFullYear(), 0); // Start of current year
                     date.setDate(parseInt(julianDateStr));
                     // Use local date parts to avoid UTC offset shifting the date (e.g. IST = UTC+5:30)
                     const y = date.getFullYear();
                     const m = String(date.getMonth() + 1).padStart(2, '0');
                     const d = String(date.getDate()).padStart(2, '0');
                     flightDate = `${y}-${m}-${d}`;
                 }

                 setIsOpen(false);
                 onScan({
                     type: 'BOARDING_PASS',
                     fullName: passengerName,
                     pnr: pnr,
                     origin: origin,
                     destination: destination,
                     flightNo: fullFlightNo,
                     flightDate: flightDate
                 });
                 return;
            }

            // If it matches neither
            setError("Unknown Document Format. Ensure you are scanning an MRZ Passport or IATA Boarding Pass.");

        } catch (e) {
            setError("Failed to parse scanned document.");
        }
    };

    const handleInput = (e: React.ChangeEvent<HTMLInputElement>) => {
        const val = e.target.value;
        const upper = val.toUpperCase();
        setScanBuffer(val);
        
        // Passports: auto-trigger at 88 chars
        const cleanLen = val.replace(/\s+/g, '').length;
        if (cleanLen >= 88 && upper.startsWith('P')) {
            processScanData(val);
        }
        // Boarding passes: auto-trigger at 50+ chars (standard BCBP is ~60+ chars)
        else if (upper.startsWith('M1') && val.length >= 50) {
            processScanData(val);
        }
    };

    const handleKeyDown = (e: React.KeyboardEvent) => {
        if (e.key === 'Enter') {
            processScanData((e.target as HTMLInputElement).value);
        }
    };

    return (
        <>
            <button 
                type="button" 
                onClick={() => setIsOpen(true)}
                className="flex items-center px-4 py-2 bg-indigo-50 text-indigo-700 border border-indigo-200 rounded-md hover:bg-indigo-100 transition-colors font-medium shadow-sm text-sm"
            >
                <Scan size={18} className="mr-2" /> Scan Document
            </button>

            {isOpen && (
                <div className="fixed inset-0 bg-slate-900/75 z-50 flex items-center justify-center p-4">
                    <div className="bg-white rounded-xl shadow-xl w-full max-w-lg border border-slate-200 overflow-hidden">
                        <div className="flex justify-between items-center p-4 border-b border-slate-100 bg-slate-50">
                            <h3 className="font-bold text-slate-800 flex items-center">
                                <Scan size={20} className="mr-2 text-indigo-600" /> Document Reader
                            </h3>
                            <button onClick={() => setIsOpen(false)} className="text-slate-400 hover:text-slate-600">
                                <X size={20} />
                            </button>
                        </div>
                        
                        <div className="p-8 flex flex-col items-center justify-center text-center">
                            <div className="w-24 h-24 bg-indigo-50 rounded-full flex items-center justify-center mb-6 border-4 border-indigo-100 animate-pulse">
                                <Scan size={40} className="text-indigo-600" />
                            </div>
                            
                            <h4 className="text-xl font-bold text-slate-800 mb-2">Ready to Scan</h4>
                            <p className="text-slate-500 max-w-sm mb-6">
                                Place the cursor in the field below and use your barcode scanner to read the <b>Passport MRZ</b> or <b>Boarding Pass</b> standard barcode.
                            </p>

                            <input 
                                ref={inputRef}
                                type="text"
                                value={scanBuffer}
                                onChange={handleInput}
                                onKeyDown={handleKeyDown}
                                className="w-full px-4 py-3 bg-slate-50 border-2 border-dashed border-indigo-300 rounded-lg focus:ring-4 focus:ring-indigo-500/20 focus:border-indigo-500 transition-colors font-mono text-center text-slate-700 placeholder:text-slate-300"
                                placeholder="Waiting for scanner input..."
                                autoFocus
                            />

                            {error && (
                                <div className="mt-4 flex items-center text-red-600 text-sm font-medium bg-red-50 px-4 py-2 rounded-md">
                                    <AlertCircle size={16} className="mr-2" /> {error}
                                </div>
                            )}
                        </div>
                    </div>
                </div>
            )}
        </>
    );
}

