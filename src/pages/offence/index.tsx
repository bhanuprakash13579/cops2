import { Routes, Route, Navigate } from 'react-router-dom';
import OffenceList from './OffenceList';
import OffenceForm from './OffenceForm';

export default function OffenceModule() {
  return (
    <Routes>
      <Route index element={<OffenceList />} />
      <Route path="new" element={<OffenceForm />} />
      <Route path="edit/:osNo" element={<OffenceForm />} />
      <Route path="*" element={<Navigate to="/offence" replace />} />
    </Routes>
  );
}
