import { Routes, Route, Navigate } from 'react-router-dom';
import { Layout } from './components/Layout';
import { Dashboard } from './components/Dashboard';
import { Proxies } from './components/Proxies';
import { Subscriptions } from './components/Subscriptions';
import { Rules } from './components/Rules';
import { Logs } from './components/Logs';
import { Settings } from './components/Settings';

function App() {
  return (
    <Layout>
      <Routes>
        <Route path="/" element={<Dashboard />} />
        <Route path="/subscriptions" element={<Subscriptions />} />
        <Route path="/proxies" element={<Proxies />} />
        <Route path="/rules" element={<Rules />} />
        <Route path="/logs" element={<Logs />} />
        <Route path="/settings" element={<Settings />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </Layout>
  );
}

export default App;
