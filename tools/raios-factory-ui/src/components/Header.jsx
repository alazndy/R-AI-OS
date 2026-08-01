import React from 'react';
import { 
  Factory, ShieldCheck, Cpu, Layers, GitPullRequest, AlertTriangle, 
  CheckCircle2, Server, Play, Pause, RefreshCw, ChevronDown, Lock, ShieldAlert
} from 'lucide-react';

export default function Header({ 
  data, 
  selectedProduct, 
  setSelectedProduct, 
  onToggleMode,
  isLiveDaemon,
  setIsLiveDaemon,
  token,
  setToken,
  onRefresh
}) {
  const { overview, products } = data;
  const isQuick = overview.mode === 'quick';

  return (
    <header className="border-b border-slate-800 bg-slate-950/80 backdrop-blur-xl sticky top-0 z-40">
      {/* Top Banner */}
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-3 flex flex-wrap items-center justify-between gap-4">
        {/* Brand & Logo */}
        <div className="flex items-center space-x-3">
          <div className="p-2.5 bg-gradient-to-br from-cyan-500/20 to-blue-600/20 border border-cyan-500/30 rounded-xl shadow-lg shadow-cyan-500/10">
            <Factory className="w-6 h-6 text-cyan-400 animate-pulse" />
          </div>
          <div>
            <div className="flex items-center space-x-2">
              <h1 className="text-xl font-bold tracking-tight text-white font-mono">
                R-AI-OS <span className="text-gradient-cyan">PRODUCT FACTORY</span>
              </h1>
              <span className="px-2 py-0.5 text-xs font-mono font-semibold bg-cyan-950/80 text-cyan-400 border border-cyan-500/30 rounded-full">
                v3.7 Kernel Engine
              </span>
            </div>
            <p className="text-xs text-slate-400">
              Owner-Bound Audit-Logged Product Lifecycle Engine (Phases 0–9)
            </p>
          </div>
        </div>

        {/* Product Selector & Operating Mode Switcher */}
        <div className="flex items-center space-x-3">
          {/* Product Dropdown */}
          <div className="relative">
            <select
              value={selectedProduct.id}
              onChange={(e) => {
                const prod = products.find(p => p.id === e.target.value);
                if (prod) setSelectedProduct(prod);
              }}
              className="appearance-none bg-slate-900 border border-slate-700 text-slate-200 text-xs font-mono rounded-lg px-3 py-2 pr-8 focus:outline-none focus:border-cyan-500 cursor-pointer"
            >
              {products.map(p => (
                <option key={p.id} value={p.id}>
                  {p.title} ({p.stack})
                </option>
              ))}
            </select>
            <ChevronDown className="w-4 h-4 text-slate-400 absolute right-2.5 top-2.5 pointer-events-none" />
          </div>

          {/* Mode Pill Toggle (Quick vs Governed) */}
          <button
            onClick={onToggleMode}
            className={`px-3 py-1.5 rounded-lg border text-xs font-mono font-semibold flex items-center space-x-1.5 transition-all ${
              isQuick
                ? 'bg-amber-950/60 border-amber-500/40 text-amber-300 hover:bg-amber-900/60'
                : 'bg-emerald-950/60 border-emerald-500/40 text-emerald-300 hover:bg-emerald-900/60'
            }`}
            title="Click to toggle Operating Mode (Governed mode enforces explicit multi-step approvals)"
          >
            {isQuick ? (
              <>
                <Zap className="w-3.5 h-3.5 text-amber-400" />
                <span>Mode: QUICK</span>
              </>
            ) : (
              <>
                <ShieldCheck className="w-3.5 h-3.5 text-emerald-400" />
                <span>Mode: GOVERNED (Strict Approval)</span>
              </>
            )}
          </button>

          {/* Session Token Input */}
          <div className="flex items-center space-x-1 bg-slate-900 border border-slate-700 rounded-lg px-2.5 py-1.5 text-xs font-mono">
            <Lock className="w-3 h-3 text-slate-400" />
            <input
              type="password"
              value={token}
              onChange={(e) => {
                setToken(e.target.value);
                localStorage.setItem('raios_session_token', e.target.value);
              }}
              placeholder="Bearer Token"
              className="bg-transparent text-cyan-300 w-24 focus:outline-none focus:w-44 transition-all"
              title="Session Token (~/.config/raios/.session_token)"
            />
          </div>

          {/* Live Daemon Connection Toggle */}
          <button
            onClick={() => setIsLiveDaemon(!isLiveDaemon)}
            className={`px-2.5 py-1.5 rounded-lg border text-xs font-mono flex items-center space-x-1.5 ${
              isLiveDaemon 
                ? 'bg-cyan-950/80 border-cyan-500/50 text-cyan-300' 
                : 'bg-slate-900 border-slate-700 text-slate-400'
            }`}
            title="Toggle Live Daemon HTTP API Stream (:42071)"
          >
            <Server className={`w-3.5 h-3.5 ${isLiveDaemon ? 'text-cyan-400 animate-pulse' : 'text-slate-500'}`} />
            <span>{isLiveDaemon ? 'HTTP: LIVE (:42071)' : 'Offline / Mock'}</span>
          </button>

          {/* Manual Refresh */}
          <button
            onClick={onRefresh}
            className="p-2 bg-slate-900 hover:bg-slate-800 border border-slate-700 text-slate-300 rounded-lg transition-colors"
            title="Refresh Factory Projections"
          >
            <RefreshCw className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      {/* KPI Cards Row */}
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-2 border-t border-slate-800/60 grid grid-cols-2 sm:grid-cols-3 md:grid-cols-6 gap-3">
        <div className="glass-card rounded-lg p-2.5 flex items-center space-x-3">
          <div className="p-2 bg-blue-500/10 border border-blue-500/20 rounded-md text-blue-400">
            <Layers className="w-4 h-4" />
          </div>
          <div>
            <div className="text-xs text-slate-400 font-mono">Products</div>
            <div className="text-base font-bold font-mono text-white">{overview.product_count}</div>
          </div>
        </div>

        <div className="glass-card rounded-lg p-2.5 flex items-center space-x-3">
          <div className="p-2 bg-cyan-500/10 border border-cyan-500/20 rounded-md text-cyan-400">
            <Cpu className="w-4 h-4" />
          </div>
          <div>
            <div className="text-xs text-slate-400 font-mono">Active Cycles</div>
            <div className="text-base font-bold font-mono text-cyan-400">{overview.active_cycle_count}</div>
          </div>
        </div>

        <div className="glass-card rounded-lg p-2.5 flex items-center space-x-3">
          <div className="p-2 bg-amber-500/10 border border-amber-500/20 rounded-md text-amber-400">
            <GitPullRequest className="w-4 h-4" />
          </div>
          <div>
            <div className="text-xs text-slate-400 font-mono">Pending CRs</div>
            <div className="text-base font-bold font-mono text-amber-400">{overview.pending_change_request_count}</div>
          </div>
        </div>

        <div className="glass-card rounded-lg p-2.5 flex items-center space-x-3">
          <div className="p-2 bg-emerald-500/10 border border-emerald-500/20 rounded-md text-emerald-400">
            <CheckCircle2 className="w-4 h-4" />
          </div>
          <div>
            <div className="text-xs text-slate-400 font-mono">Verified Stages</div>
            <div className="text-base font-bold font-mono text-emerald-400">{overview.completed_verify_stages}</div>
          </div>
        </div>

        <div className="glass-card rounded-lg p-2.5 flex items-center space-x-3">
          <div className="p-2 bg-rose-500/10 border border-rose-500/20 rounded-md text-rose-400">
            <ShieldAlert className="w-4 h-4" />
          </div>
          <div>
            <div className="text-xs text-slate-400 font-mono">Release Blockers</div>
            <div className="text-base font-bold font-mono text-rose-400">{selectedProduct.release_blockers}</div>
          </div>
        </div>

        <div className="glass-card rounded-lg p-2.5 flex items-center space-x-3">
          <div className="p-2 bg-violet-500/10 border border-violet-500/20 rounded-md text-violet-400">
            <Lock className="w-4 h-4" />
          </div>
          <div>
            <div className="text-xs text-slate-400 font-mono">Release Drafts</div>
            <div className="text-base font-bold font-mono text-violet-300">{overview.release_drafts}</div>
          </div>
        </div>
      </div>
    </header>
  );
}
