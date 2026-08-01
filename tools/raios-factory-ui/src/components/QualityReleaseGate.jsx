import React from 'react';
import { 
  ShieldCheck, AlertTriangle, CheckCircle2, XCircle, Smartphone, 
  Terminal, Package, Award, ArrowUpRight, Lock, Check
} from 'lucide-react';

export default function QualityReleaseGate({ 
  data, 
  selectedProduct,
  onApproveRelease
}) {
  const { qualityProfiles, releaseDrafts } = data;
  const releaseDraft = releaseDrafts[0];

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="glass-panel p-6 rounded-2xl flex flex-wrap items-center justify-between gap-4">
        <div>
          <h2 className="text-lg font-bold text-white flex items-center space-x-2 font-mono">
            <ShieldCheck className="w-5 h-5 text-emerald-400" />
            <span>QUALITY PROFILES & RELEASE READINESS GATE</span>
          </h2>
          <p className="text-xs text-slate-400">
            Phase 7 Closed-Testing Quality Profiles & Phase 8 Release Sign-off for <span className="text-cyan-300 font-mono">{selectedProduct.title}</span>
          </p>
        </div>

        <div className="flex items-center space-x-2">
          <span className="text-xs font-mono px-3 py-1 bg-emerald-950 border border-emerald-500/40 text-emerald-400 rounded-full font-bold">
            Quality Gate: MANDATORY
          </span>
        </div>
      </div>

      {/* Quality Profiles Cards */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {qualityProfiles.map((profile) => (
          <div key={profile.id} className="glass-panel p-6 rounded-2xl space-y-4">
            <div className="flex items-center justify-between border-b border-slate-800 pb-3">
              <div>
                <span className="text-[10px] font-mono text-cyan-400 font-bold uppercase tracking-wider">
                  Quality Profile #{profile.id}
                </span>
                <h3 className="text-sm font-bold text-white font-mono mt-0.5">
                  {profile.name}
                </h3>
              </div>
              <span className="px-2 py-0.5 text-[10px] font-mono bg-cyan-950 text-cyan-400 border border-cyan-500/30 rounded">
                Required: Yes
              </span>
            </div>

            {/* Checklist Table */}
            <div className="space-y-2">
              {profile.checks.map((chk, idx) => (
                <div 
                  key={idx} 
                  className={`p-3 rounded-xl border flex items-start justify-between text-xs font-mono ${
                    chk.passed 
                      ? 'bg-slate-900/60 border-slate-800' 
                      : 'bg-rose-950/40 border-rose-500/40'
                  }`}
                >
                  <div className="space-y-1">
                    <div className="font-bold text-white flex items-center space-x-2">
                      {chk.passed ? (
                        <CheckCircle2 className="w-4 h-4 text-emerald-400 shrink-0" />
                      ) : (
                        <XCircle className="w-4 h-4 text-rose-400 shrink-0 animate-pulse" />
                      )}
                      <span>{chk.name}</span>
                    </div>
                    <div className="text-[11px] text-slate-400 pl-6">
                      Evidence: <code className="text-slate-300">{chk.evidence}</code>
                    </div>
                  </div>

                  <span className={`text-[10px] font-mono font-bold px-2 py-0.5 rounded ${
                    chk.passed ? 'bg-emerald-950 text-emerald-400' : 'bg-rose-950 text-rose-400'
                  }`}>
                    {chk.passed ? 'PASSED' : 'BLOCKING'}
                  </span>
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>

      {/* Release Draft & Approval Panel */}
      {releaseDraft && (
        <div className="glass-panel p-6 rounded-2xl space-y-4 bg-gradient-to-r from-slate-950 via-slate-900 to-slate-950 border-cyan-500/30">
          <div className="flex flex-wrap items-center justify-between gap-4 border-b border-slate-800 pb-4">
            <div>
              <span className="text-xs font-mono text-violet-400 font-bold uppercase tracking-wider">
                Release Draft Candidate
              </span>
              <h3 className="text-lg font-bold text-white font-mono mt-0.5 flex items-center space-x-2">
                <Package className="w-5 h-5 text-violet-400" />
                <span>{releaseDraft.version}</span>
              </h3>
              <p className="text-xs text-slate-400 font-mono mt-0.5">
                Build SHA-256: <code className="text-cyan-300">{releaseDraft.buildRef}</code>
              </p>
            </div>

            <button
              onClick={() => onApproveRelease(releaseDraft.id)}
              disabled={releaseDraft.blockers.length > 0}
              className={`px-5 py-2.5 font-bold font-mono text-xs rounded-xl shadow-lg transition-all flex items-center space-x-2 ${
                releaseDraft.blockers.length === 0
                  ? 'bg-gradient-to-r from-emerald-500 to-teal-500 hover:from-emerald-400 hover:to-teal-400 text-black shadow-emerald-500/20 cursor-pointer'
                  : 'bg-slate-800 text-slate-500 cursor-not-allowed border border-slate-700'
              }`}
            >
              <Award className="w-4 h-4" />
              <span>Approve Closed-Testing Release</span>
            </button>
          </div>

          {/* Release Blockers */}
          {releaseDraft.blockers.length > 0 ? (
            <div className="bg-rose-950/40 border border-rose-500/40 rounded-xl p-4 space-y-2">
              <h4 className="text-xs font-mono uppercase tracking-wider text-rose-400 font-bold flex items-center space-x-2">
                <AlertTriangle className="w-4 h-4 text-rose-400" />
                <span>Release Sign-off Blocked ({releaseDraft.blockers.length})</span>
              </h4>
              <ul className="list-disc list-inside text-xs font-mono text-rose-200 space-y-1">
                {releaseDraft.blockers.map((blk, idx) => (
                  <li key={idx}>{blk}</li>
                ))}
              </ul>
            </div>
          ) : (
            <div className="bg-emerald-950/40 border border-emerald-500/40 rounded-xl p-4 text-xs font-mono text-emerald-300 flex items-center space-x-2">
              <CheckCircle2 className="w-4 h-4 text-emerald-400" />
              <span>All quality profiles green. Release draft ready for human approval sign-off.</span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
