import React from 'react';
import { 
  LifeBuoy, MessageSquare, Bug, CheckCircle2, GitPullRequest, ArrowRight, 
  ExternalLink, UserCheck
} from 'lucide-react';

export default function SupportTriageDesk({ data, selectedProduct, onTriageSupportItem }) {
  const { supportItems } = data;

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="glass-panel p-6 rounded-2xl flex flex-wrap items-center justify-between gap-4">
        <div>
          <h2 className="text-lg font-bold text-white flex items-center space-x-2 font-mono">
            <LifeBuoy className="w-5 h-5 text-purple-400" />
            <span>SUPPORT & FEEDBACK TRIAGE DESK</span>
          </h2>
          <p className="text-xs text-slate-400">
            Phase 9 Owner-Bound Feedback Loop & Change Request Linking
          </p>
        </div>

        <button
          onClick={() => {
            const summary = prompt('Enter support item summary:');
            if (summary) {
              alert(`Created Support Item: "${summary}"`);
            }
          }}
          className="px-4 py-2 bg-purple-600 hover:bg-purple-500 text-white font-bold font-mono text-xs rounded-xl shadow-lg shadow-purple-600/20 transition-all"
        >
          + Create Support Ticket
        </button>
      </div>

      {/* Support Items Grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {supportItems.map((item) => {
          const isBug = item.sourceKind === 'bug';
          const isResolved = item.status === 'resolved';

          return (
            <div key={item.id} className="glass-panel p-5 rounded-2xl space-y-4">
              <div className="flex items-center justify-between border-b border-slate-800 pb-3">
                <div className="flex items-center space-x-2">
                  <span className="text-xs font-mono font-bold text-purple-400">
                    {item.id.toUpperCase()}
                  </span>
                  <span className={`text-[10px] font-mono font-bold px-2 py-0.5 rounded ${
                    isBug ? 'bg-rose-950 text-rose-400 border border-rose-500/30' : 'bg-indigo-950 text-indigo-300 border border-indigo-500/30'
                  }`}>
                    {item.sourceKind.toUpperCase()}
                  </span>
                </div>

                <span className={`text-[10px] font-mono font-bold px-2 py-0.5 rounded-full ${
                  isResolved ? 'bg-emerald-950 text-emerald-400' : 'bg-amber-950 text-amber-400'
                }`}>
                  {item.status.toUpperCase()}
                </span>
              </div>

              <h4 className="text-xs font-bold text-white font-sans">
                {item.summary}
              </h4>

              <div className="text-xs font-mono text-slate-400 space-y-1">
                <div>Submitted: <span className="text-slate-300">{item.createdAt}</span></div>
                {item.linkedChangeRequestId && (
                  <div className="text-cyan-400 font-bold flex items-center space-x-1">
                    <GitPullRequest className="w-3.5 h-3.5" />
                    <span>Linked CR: {item.linkedChangeRequestId.toUpperCase()}</span>
                  </div>
                )}
                {item.resolutionRef && (
                  <div className="text-emerald-400 font-bold flex items-center space-x-1">
                    <CheckCircle2 className="w-3.5 h-3.5" />
                    <span>Resolution: {item.resolutionRef}</span>
                  </div>
                )}
              </div>

              {!isResolved && (
                <div className="pt-2 flex justify-end">
                  <button
                    onClick={() => onTriageSupportItem(item.id)}
                    className="px-3 py-1.5 bg-slate-900 hover:bg-slate-800 text-purple-300 border border-purple-500/30 font-mono text-xs font-bold rounded-xl transition-all flex items-center space-x-1"
                  >
                    <UserCheck className="w-3.5 h-3.5" />
                    <span>Triage & Link to CR</span>
                  </button>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
