import { Routes, Route, Navigate } from 'react-router-dom';
import { Layout } from './components/Layout';
import { Dashboard } from './components/Dashboard';
import { Proxies } from './components/Proxies';
import { Subscriptions } from './components/Subscriptions';
import { Rules } from './components/Rules';
import { Activity } from 'lucide-react';

const DevelopingPlaceholder = ({ name }: { name: string }) => (
  <div className="flex flex-col items-center justify-center h-[60vh] text-center space-y-6">
    <div className="size-24 bg-muted rounded-[2rem] flex items-center justify-center shadow-inner">
      <Activity className="size-12 text-muted-foreground animate-pulse" />
    </div>
    <div className="space-y-2">
      <h3 className="text-2xl font-black uppercase tracking-tighter text-muted-foreground italic">"{name}"</h3>
      <p className="text-sm font-bold text-muted-foreground/40 uppercase tracking-widest">Module Under Development</p>
    </div>
  </div>
);

function App() {
  return (
    <Layout>
      <Routes>
        <Route path="/" element={<Dashboard />} />
        <Route path="/subscriptions" element={<Subscriptions />} />
        <Route path="/proxies" element={<Proxies />} />
        <Route path="/rules" element={<Rules />} />
        <Route path="/logs" element={<DevelopingPlaceholder name="Running Logs" />} />
        <Route path="/settings" element={<DevelopingPlaceholder name="System Settings" />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </Layout>
  );
}

export default App;
