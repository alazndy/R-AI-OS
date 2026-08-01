import React, { useState } from 'react';
import { Terminal, ChevronUp, ChevronDown, Copy, Check, Play } from 'lucide-react';

export default function CommandTerminal({ logs, selectedProduct }) {
  const [isOpen, setIsOpen] = useState(true);
  const [customCmd, setCustomCmd] = useState('');
  const [copiedIdx, setCopiedIdx] = useState(null);

  const handleCopy = (text, idx) => {
    navigator.clipboard.writeText(text);
    setCopiedIdx(idx);
    setTimeout(() => setCopiedIdx(null), 2000);
  };

  return (
    <div className="fixed bottom-0 left-0 right-0 z-30 bg-slate-950 border-t border-slate-800 shadow-2xl transition-all">
      {/* Terminal Bar Toggle Header */}
      <button
        onClick={() => setIsOpen(!isOpen)}
        className="w-full px-4 py-2 bg-slate-900/90 hover:bg-slate-850 border-b border-slate-800 flex items-center justify-between text-xs font-mono text-slate-300 transition-colors"
      >
        <div className="flex items-center space-x-2">
          <Terminal className="w-4 h-4 text-cyan-400" />
          <span className="font-bold text-white">R-AI-OS CONTROL-PLANE IPC TERMINAL</span>
          <span className="px-2 py-0.5 bg-slate-800 text-slate-400 rounded text-[10px]">
            {logs.length} Executions Logged
          </span>
        </div>

        <div className="flex items-center space-x-3">
          <span className="text-[11px] text-slate-400">
            Target: <code className="text-cyan-300">{selectedProduct.id}</code>
          </span>
          {isOpen ? <ChevronDown className="w-4 h-4" /> : <ChevronUp className="w-4 h-4" />}
        </div>
      </button>

      {/* Terminal Drawer Body */}
      {isOpen && (
        <div className="p-4 max-w-7xl mx-auto space-y-3">
          {/* Logs List */}
          <div className="bg-slate-900/90 border border-slate-800 rounded-xl p-3 max-h-36 overflow-y-auto font-mono text-xs space-y-1.5">
            {logs.map((log, idx) => (
              <div key={idx} className="flex items-center justify-between group hover:bg-slate-800/50 p-1 rounded">
                <div className="flex items-center space-x-2">
                  <span className="text-[10px] text-slate-500">{log.timestamp}</span>
                  <span className="text-cyan-400 font-bold">$</span>
                  <code className="text-slate-200">{log.cmd}</code>
                </div>

                <div className="flex items-center space-x-2">
                  <span className="text-[10px] text-emerald-400">{log.status} ({log.latency})</span>
                  <button
                    onClick={() => handleCopy(log.cmd, idx)}
                    className="p-1 text-slate-400 hover:text-cyan-400 opacity-0 group-hover:opacity-100 transition-opacity"
                    title="Copy command"
                  >
                    {copiedIdx === idx ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
                  </button>
                </div>
              </div>
            ))}
          </div>

          {/* Interactive Shell Input */}
          <form
            onSubmit={(e) => {
              e.preventDefault();
              if (customCmd) {
                alert(`Executed command over daemon IPC: ${customCmd}`);
                setCustomCmd('');
              }
            }}
            className="flex items-center space-x-2"
          >
            <div className="relative flex-grow">
              <span className="absolute left-3 top-2.5 text-cyan-400 font-mono font-bold text-xs">$</span>
              <input
                type="text"
                placeholder="raios factory execute CreateProductDraft --title 'Pilot' ..."
                value={customCmd}
                onChange={(e) => setCustomCmd(e.target.value)}
                className="w-full bg-slate-900 border border-slate-800 text-slate-200 font-mono text-xs pl-7 pr-3 py-2 rounded-xl focus:outline-none focus:border-cyan-500"
              />
            </div>

            <button
              type="submit"
              className="px-4 py-2 bg-cyan-600 hover:bg-cyan-500 text-black font-bold font-mono text-xs rounded-xl transition-all flex items-center space-x-1.5"
            >
              <Play className="w-3.5 h-3.5 fill-black" />
              <span>Dispatch</span>
            </button>
          </form>
        </div>
      )}
    </div>
  );
}
