import React, { useState } from 'react';
import { 
  FileText, HelpCircle, CheckCircle2, Send, Edit3, Shield, Sparkles, 
  ChevronRight, Bookmark, Lock, RefreshCw, AlertCircle
} from 'lucide-react';

export default function IntakeCharterStudio({ data, selectedProduct, onSaveCharter, onAnswerQuestion }) {
  const { intakeSession, charter, requirements } = data;
  const [answers, setAnswers] = useState(intakeSession.answers);
  const [activeTab, setActiveTab] = useState('charter'); // 'charter' | 'intake' | 'requirements'
  const [charterContent, setCharterContent] = useState(charter.content);
  const [newQuestionKey, setNewQuestionKey] = useState('');
  const [newAnswerText, setNewAnswerText] = useState('');

  return (
    <div className="space-y-6">
      {/* Studio Navigation Sub-Tabs */}
      <div className="flex items-center justify-between border-b border-slate-800 pb-3">
        <div className="flex items-center space-x-2">
          <button
            onClick={() => setActiveTab('charter')}
            className={`px-4 py-2 rounded-xl text-xs font-mono font-bold transition-all flex items-center space-x-2 ${
              activeTab === 'charter'
                ? 'bg-cyan-950/80 border border-cyan-500/40 text-cyan-300 shadow-md shadow-cyan-500/10'
                : 'bg-slate-900 border border-slate-800 text-slate-400 hover:text-slate-200'
            }`}
          >
            <FileText className="w-4 h-4" />
            <span>01 | Charter Document (Rev {charter.revision})</span>
          </button>

          <button
            onClick={() => setActiveTab('intake')}
            className={`px-4 py-2 rounded-xl text-xs font-mono font-bold transition-all flex items-center space-x-2 ${
              activeTab === 'intake'
                ? 'bg-cyan-950/80 border border-cyan-500/40 text-cyan-300 shadow-md shadow-cyan-500/10'
                : 'bg-slate-900 border border-slate-800 text-slate-400 hover:text-slate-200'
            }`}
          >
            <HelpCircle className="w-4 h-4" />
            <span>02 | Intake Questionnaire ({answers.length} Answered)</span>
          </button>

          <button
            onClick={() => setActiveTab('requirements')}
            className={`px-4 py-2 rounded-xl text-xs font-mono font-bold transition-all flex items-center space-x-2 ${
              activeTab === 'requirements'
                ? 'bg-cyan-950/80 border border-cyan-500/40 text-cyan-300 shadow-md shadow-cyan-500/10'
                : 'bg-slate-900 border border-slate-800 text-slate-400 hover:text-slate-200'
            }`}
          >
            <Bookmark className="w-4 h-4" />
            <span>03 | Requirements Drafts ({requirements.length})</span>
          </button>
        </div>

        <div className="text-xs font-mono text-slate-400">
          Target Product: <span className="text-cyan-300 font-bold">{selectedProduct.title}</span>
        </div>
      </div>

      {/* Tab 1: Charter Markdown Editor & Viewer */}
      {activeTab === 'charter' && (
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
          {/* Editor Column */}
          <div className="glass-panel p-5 rounded-2xl space-y-4">
            <div className="flex items-center justify-between border-b border-slate-800 pb-3">
              <h3 className="text-sm font-bold text-white font-mono flex items-center space-x-2">
                <Edit3 className="w-4 h-4 text-cyan-400" />
                <span>Markdown Charter Editor</span>
              </h3>
              <span className="text-xs font-mono text-emerald-400 flex items-center space-x-1">
                <CheckCircle2 className="w-3.5 h-3.5" />
                <span>Status: {charter.status.toUpperCase()}</span>
              </span>
            </div>

            <textarea
              value={charterContent}
              onChange={(e) => setCharterContent(e.target.value)}
              rows={16}
              className="w-full bg-slate-950 border border-slate-800 text-slate-200 font-mono text-xs p-4 rounded-xl focus:outline-none focus:border-cyan-500 transition-colors leading-relaxed resize-none"
            />

            <div className="flex items-center justify-between pt-2">
              <span className="text-xs text-slate-400 font-mono">
                Approved By: <span className="text-slate-200">{charter.approvedBy}</span>
              </span>

              <button
                onClick={() => onSaveCharter(charterContent)}
                className="px-4 py-2 bg-emerald-600 hover:bg-emerald-500 text-black font-bold font-mono text-xs rounded-xl shadow-lg shadow-emerald-600/20 transition-all flex items-center space-x-2"
              >
                <Shield className="w-4 h-4" />
                <span>Save & Approve Revision</span>
              </button>
            </div>
          </div>

          {/* Rendered Charter Preview Column */}
          <div className="glass-panel p-5 rounded-2xl space-y-4 bg-slate-900/60">
            <div className="flex items-center justify-between border-b border-slate-800 pb-3">
              <h3 className="text-sm font-bold text-white font-mono flex items-center space-x-2">
                <FileText className="w-4 h-4 text-emerald-400" />
                <span>Rendered Charter Document</span>
              </h3>
              <span className="text-xs font-mono text-slate-400">Preview Mode</span>
            </div>

            <div className="prose prose-invert max-w-none text-xs text-slate-300 font-sans space-y-3 p-4 bg-slate-950/70 border border-slate-800 rounded-xl max-h-[420px] overflow-y-auto">
              <pre className="whitespace-pre-wrap font-mono text-slate-300 text-xs bg-transparent p-0 border-0">
                {charterContent}
              </pre>
            </div>
          </div>
        </div>
      )}

      {/* Tab 2: Intake Questionnaire Runner */}
      {activeTab === 'intake' && (
        <div className="glass-panel p-6 rounded-2xl space-y-6">
          <div className="flex items-center justify-between border-b border-slate-800 pb-4">
            <div>
              <h3 className="text-base font-bold text-white font-mono flex items-center space-x-2">
                <HelpCircle className="w-5 h-5 text-cyan-400" />
                <span>PRODUCT INTAKE QUESTIONNAIRE</span>
              </h3>
              <p className="text-xs text-slate-400">
                Interactive discovery session for <span className="text-cyan-300 font-mono">{selectedProduct.title}</span>
              </p>
            </div>
            <button
              onClick={() => {
                alert('Auto-generating Charter draft from completed intake answers...');
              }}
              className="px-4 py-2 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-500 hover:to-indigo-500 text-white font-bold font-mono text-xs rounded-xl shadow-lg shadow-purple-500/20 transition-all flex items-center space-x-2"
            >
              <Sparkles className="w-4 h-4" />
              <span>Auto-Generate Charter</span>
            </button>
          </div>

          {/* Answered Questions List */}
          <div className="space-y-4">
            <h4 className="text-xs font-mono uppercase tracking-wider text-slate-400 font-bold">
              Recorded Answers ({answers.length})
            </h4>

            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              {answers.map((ans, idx) => (
                <div key={idx} className="glass-card p-4 rounded-xl border-slate-800 space-y-2">
                  <div className="text-xs font-bold text-cyan-400 font-mono flex items-center space-x-2">
                    <CheckCircle2 className="w-3.5 h-3.5 text-emerald-400" />
                    <span>Q: {ans.label}</span>
                  </div>
                  <p className="text-xs text-slate-200 font-sans pl-5 border-l-2 border-cyan-500/40">
                    {ans.response}
                  </p>
                </div>
              ))}
            </div>
          </div>

          {/* Interactive Answer Input Form */}
          <div className="bg-slate-900/90 border border-slate-800 rounded-xl p-4 space-y-3">
            <h4 className="text-xs font-mono uppercase tracking-wider text-cyan-400 font-bold">
              Record New Intake Response
            </h4>

            <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
              <input
                type="text"
                placeholder="Question key (e.g. telemetry_policy)"
                value={newQuestionKey}
                onChange={(e) => setNewQuestionKey(e.target.value)}
                className="bg-slate-950 border border-slate-800 text-slate-200 text-xs font-mono px-3 py-2 rounded-lg focus:outline-none focus:border-cyan-500"
              />
              <input
                type="text"
                placeholder="Answer response..."
                value={newAnswerText}
                onChange={(e) => setNewAnswerText(e.target.value)}
                className="bg-slate-950 border border-slate-800 text-slate-200 text-xs font-mono px-3 py-2 rounded-lg focus:outline-none focus:border-cyan-500 md:col-span-2"
              />
            </div>

            <div className="flex justify-end">
              <button
                onClick={() => {
                  if (newQuestionKey && newAnswerText) {
                    onAnswerQuestion(newQuestionKey, newAnswerText);
                    setAnswers([...answers, { key: newQuestionKey, label: newQuestionKey, response: newAnswerText }]);
                    setNewQuestionKey('');
                    setNewAnswerText('');
                  }
                }}
                className="px-4 py-2 bg-cyan-600 hover:bg-cyan-500 text-black font-bold font-mono text-xs rounded-xl shadow-md transition-all flex items-center space-x-2"
              >
                <Send className="w-3.5 h-3.5" />
                <span>Submit Intake Answer</span>
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Tab 3: Requirements Drafts Grid */}
      {activeTab === 'requirements' && (
        <div className="glass-panel p-6 rounded-2xl space-y-6">
          <div className="flex items-center justify-between border-b border-slate-800 pb-4">
            <div>
              <h3 className="text-base font-bold text-white font-mono flex items-center space-x-2">
                <Bookmark className="w-5 h-5 text-emerald-400" />
                <span>REQUIREMENTS DRAFT MATRIX</span>
              </h3>
              <p className="text-xs text-slate-400">
                Stable REQ-xxx keys backed by content-addressed evidence
              </p>
            </div>

            <button
              onClick={() => {
                const title = prompt('Enter requirement title:');
                if (title) {
                  alert(`Created Requirement REQ-00${requirements.length + 1}: ${title}`);
                }
              }}
              className="px-4 py-2 bg-emerald-600 hover:bg-emerald-500 text-black font-bold font-mono text-xs rounded-xl shadow-lg transition-all"
            >
              + Create Requirement Draft
            </button>
          </div>

          {/* Requirements Cards */}
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            {requirements.map((req) => (
              <div key={req.id} className="glass-card p-4 rounded-xl border-slate-800 space-y-3">
                <div className="flex items-center justify-between">
                  <span className="text-xs font-mono font-bold text-cyan-400 bg-cyan-950/80 px-2 py-0.5 rounded border border-cyan-500/30">
                    {req.key}
                  </span>
                  <span className={`text-[10px] font-mono font-bold px-2 py-0.5 rounded-full ${
                    req.status === 'approved' 
                      ? 'bg-emerald-950 text-emerald-400 border border-emerald-500/30' 
                      : 'bg-amber-950 text-amber-400 border border-amber-500/30'
                  }`}>
                    {req.status.toUpperCase()}
                  </span>
                </div>

                <h4 className="text-xs font-semibold text-white">
                  {req.title}
                </h4>

                <div className="flex items-center justify-between pt-2 text-[11px] font-mono text-slate-400 border-t border-slate-850">
                  <span>Priority: <strong className="text-slate-200">{req.priority}</strong></span>
                  <span>Evidence: <strong className="text-emerald-400">{req.evidenceCount} Linked</strong></span>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
