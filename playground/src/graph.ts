// IR Graph SVG renderer — extracted from debug_ui.html
// Renders a topologically-sorted, layered dataflow graph with Bezier edges.

import type { IrData } from "./types";

// --- Constants ---
const NW = 160, NODEH = 50, GX = 60, GY = 90, PAD = 40;
const PORT_R = 5, PORT_GAP = 20;

// --- Types ---

interface NodeInfo {
  type: "input" | "output" | "source" | "sink" | "node";
  label: string;
  sub?: string;
  takes: string[];
  emits: string[];
}

interface EdgeInfo {
  from: string;
  to: string;
  varName: string | null;
  takeIdx: number;
  when: string;
}

interface NodePos {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface GraphState {
  currentNodeId: string | null;
  executedNodes: Set<string>;
  failedNodes: Set<string>;
  iterationExecuted: Set<string>;
  breakpoints: Set<string>;
  bindings: Record<string, unknown>;
}

// --- Helpers ---

function resolveEdgeId(ep: { kind?: string; id: string }): string {
  if (ep.kind === "source_event") return "src:" + ep.id;
  if (ep.kind === "input") return "in:" + ep.id;
  if (ep.kind === "output") return "out:" + ep.id;
  return ep.id;
}

function extractBranchLabel(when: string): string | null {
  const m = when.match(/case\(\((.+?)\)\s*==\s*true\)/);
  return m ? m[1] : null;
}

function fmtVal(v: unknown): string {
  if (v === undefined || v === null) return "";
  if (typeof v === "string") return v.length > 16 ? v.slice(0, 14) + ".." : v;
  if (typeof v === "number") {
    const s = String(v);
    return s.length > 12 ? v.toPrecision(6) : s;
  }
  if (typeof v === "boolean") return String(v);
  const s = JSON.stringify(v);
  return s.length > 16 ? s.slice(0, 14) + ".." : s;
}

function esc(s: string): string {
  return String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

function computeInactive(
  edgeList: EdgeInfo[],
  executedSet: Set<string>,
  curNodeId: string | null
): Set<string> {
  const inactive = new Set<string>();
  const byFrom: Record<string, EdgeInfo[]> = {};
  edgeList.forEach((e) => {
    if (!byFrom[e.from]) byFrom[e.from] = [];
    byFrom[e.from].push(e);
  });

  for (const [, edges] of Object.entries(byFrom)) {
    const condEdges = edges.filter((e) => extractBranchLabel(e.when));
    if (condEdges.length < 2) continue;
    const takenTargets = condEdges.filter(
      (e) => executedSet.has(e.to) || e.to === curNodeId
    );
    if (takenTargets.length === 0) continue;
    condEdges.forEach((e) => {
      if (!takenTargets.includes(e)) inactive.add(e.to);
    });
  }

  // BFS propagation
  let changed = true;
  while (changed) {
    changed = false;
    edgeList.forEach((e) => {
      if (
        inactive.has(e.from) &&
        !executedSet.has(e.to) &&
        e.to !== curNodeId &&
        !inactive.has(e.to)
      ) {
        inactive.add(e.to);
        changed = true;
      }
    });
  }
  return inactive;
}

function emitPortPos(
  pos: NodePos,
  emitIdx: number,
  totalEmits: number
): { x: number; y: number } {
  const spacing = Math.min(PORT_GAP, pos.w / (totalEmits + 1));
  const startX = pos.x + pos.w / 2 - (totalEmits - 1) * spacing / 2;
  return { x: startX + emitIdx * spacing, y: pos.y + pos.h + PORT_R + 1 };
}

function takePortPos(
  pos: NodePos,
  takeIdx: number,
  totalTakes: number
): { x: number; y: number } {
  const spacing = Math.min(PORT_GAP, pos.w / (totalTakes + 1));
  const startX = pos.x + pos.w / 2 - (totalTakes - 1) * spacing / 2;
  return { x: startX + takeIdx * spacing, y: pos.y - PORT_R - 1 };
}

function bary(nodeId: string, edges: EdgeInfo[], prevIdx: Record<string, number>): number {
  let sum = 0, count = 0;
  edges.forEach((e) => {
    if (e.to === nodeId && e.from in prevIdx) {
      sum += prevIdx[e.from];
      count++;
    }
  });
  return count ? sum / count : 0;
}

// --- Main render function ---

export function renderGraph(
  ir: IrData,
  container: HTMLElement,
  state?: GraphState,
  onNodeClick?: (nodeId: string) => void
): void {
  if (!ir || !ir.nodes) {
    container.innerHTML = '<div style="display:flex;align-items:center;justify-content:center;height:100%;color:#8b949e;font-size:13px">No IR available</div>';
    return;
  }

  const currentNodeId = state?.currentNodeId ?? null;
  const executedNodeSet = state?.executedNodes ?? new Set<string>();
  const failedNodeSet = state?.failedNodes ?? new Set<string>();
  const iterationExecutedSet = state?.iterationExecuted ?? new Set<string>();
  const breakpoints = state?.breakpoints ?? new Set<string>();
  const currentBindings = state?.bindings ?? {};

  const nodeMap: Record<string, NodeInfo> = {};
  const allIds: string[] = [];

  // 1. Input ports
  const inputs = (ir as Record<string, unknown>).inputs as { name: string }[] | undefined;
  if (inputs) {
    inputs.forEach((p) => {
      const id = "in:" + p.name;
      allIds.push(id);
      nodeMap[id] = { type: "input", label: p.name, takes: [], emits: [p.name] };
    });
  }

  // 2. Source nodes from source_event edges
  const edges = ir.edges as { from: { kind?: string; id: string }; to: { kind?: string; id: string }; when: string }[] | undefined;
  const srcSeen = new Map<string, string>();
  if (edges) {
    edges.forEach((e) => {
      if (e.from.kind === "source_event" && !srcSeen.has(e.from.id)) {
        const m = e.when.match(/source_loop\(([^ ]+) as ([^)]+)\)/);
        srcSeen.set(e.from.id, m ? m[1] : "source");
      }
    });
  }
  srcSeen.forEach((opName, varName) => {
    const id = "src:" + varName;
    allIds.push(id);
    nodeMap[id] = { type: "source", label: opName, sub: varName, takes: [], emits: [varName] };
  });

  // 3. Computation nodes
  const inDeg: Record<string, number> = {};
  ir.nodes!.forEach((n) => {
    allIds.push(n.id);
    const isSink = /^sinks?\./.test(n.op);
    const takes: string[] = [];
    const args = (n as Record<string, unknown>).args as { var?: string }[] | undefined;
    if (args) {
      args.forEach((a) => {
        if (a.var) takes.push(a.var);
      });
    }
    const bind = (n as Record<string, unknown>).bind as string | undefined;
    nodeMap[n.id] = {
      type: isSink ? "sink" : "node",
      label: n.op,
      sub: bind,
      takes,
      emits: [bind || n.id],
    };
    inDeg[n.id] = 0;
  });

  // 4. Output ports
  const outputs = (ir as Record<string, unknown>).outputs as { name: string }[] | undefined;
  if (outputs) {
    outputs.forEach((p) => {
      const id = "out:" + p.name;
      allIds.push(id);
      nodeMap[id] = { type: "output", label: p.name, takes: [p.name], emits: [] };
    });
  }

  // Init adjacency + in-degree
  const adj: Record<string, string[]> = {};
  allIds.forEach((id) => {
    adj[id] = [];
    if (!(id in inDeg)) inDeg[id] = 0;
  });

  // Build edge list
  const edgeList: EdgeInfo[] = [];
  if (edges) {
    edges.forEach((e) => {
      const from = resolveEdgeId(e.from);
      const to = resolveEdgeId(e.to);
      let varName: string | null = null;
      if (e.from.kind === "source_event") varName = e.from.id;
      else {
        const fn = nodeMap[from];
        if (fn && fn.emits.length > 0) varName = fn.emits[0];
      }
      const tn = nodeMap[to];
      let tidx = 0;
      if (tn && varName) {
        const ti = tn.takes.indexOf(varName);
        if (ti >= 0) tidx = ti;
      }
      edgeList.push({ from, to, varName, takeIdx: tidx, when: e.when });
      if (adj[from]) adj[from].push(to);
      if (to in inDeg) inDeg[to]++;
      else inDeg[to] = 1;
    });
  }

  // Topological layering
  const layer: Record<string, number> = {};
  const queue = allIds.filter((id) => (inDeg[id] || 0) === 0);
  queue.forEach((id) => { layer[id] = 0; });
  const visited = new Set<string>();
  while (queue.length) {
    const id = queue.shift()!;
    if (visited.has(id)) continue;
    visited.add(id);
    const l = layer[id] || 0;
    (adj[id] || []).forEach((to) => {
      layer[to] = Math.max(layer[to] || 0, l + 1);
      inDeg[to]--;
      if (inDeg[to] <= 0) queue.push(to);
    });
  }
  allIds.forEach((id) => { if (!(id in layer)) layer[id] = 0; });

  const maxLayer = Math.max(0, ...Object.values(layer));
  const byLayer: string[][] = [];
  for (let i = 0; i <= maxLayer; i++) byLayer.push([]);
  allIds.forEach((id) => byLayer[layer[id]].push(id));

  // Barycenter ordering (3-pass)
  for (let pass = 0; pass < 3; pass++) {
    for (let l = 1; l <= maxLayer; l++) {
      const prev = byLayer[l - 1];
      const prevIdx: Record<string, number> = {};
      prev.forEach((id, i) => { prevIdx[id] = i; });
      byLayer[l].sort((a, b) => bary(a, edgeList, prevIdx) - bary(b, edgeList, prevIdx));
    }
  }

  // Compute positions
  const nodePositions: Record<string, NodePos> = {};
  const maxInLayer = Math.max(1, ...byLayer.map((l) => l.length));
  const W = maxInLayer * (NW + GX) - GX + PAD * 2;
  const H = (maxLayer + 1) * (NODEH + GY) - GY + PAD * 2;

  byLayer.forEach((ids, l) => {
    const rowW = ids.length * (NW + GX) - GX;
    const xOff = (W - rowW) / 2;
    ids.forEach((id, i) => {
      nodePositions[id] = { x: xOff + i * (NW + GX), y: PAD + l * (NODEH + GY), w: NW, h: NODEH };
    });
  });

  // Compute inactive nodes
  const inactive = computeInactive(edgeList, iterationExecutedSet, currentNodeId);

  // Active source nodes
  const activeSourceIds = new Set<string>();
  edgeList.forEach((e) => {
    if (e.to === currentNodeId && nodeMap[e.from]?.type === "source") {
      activeSourceIds.add(e.from);
    }
  });

  // Connected ports
  const connectedEmits = new Set<string>();
  const connectedTakes = new Set<string>();
  edgeList.forEach((e) => {
    if (e.varName) connectedEmits.add(e.from + ":" + e.varName);
    const tn = nodeMap[e.to];
    if (tn && tn.takes[e.takeIdx]) connectedTakes.add(e.to + ":" + tn.takes[e.takeIdx]);
  });

  // Fork points
  const outgoing: Record<string, EdgeInfo[]> = {};
  edgeList.forEach((e) => {
    if (!outgoing[e.from]) outgoing[e.from] = [];
    outgoing[e.from].push(e);
  });
  const forkPoints = new Set<string>();
  Object.entries(outgoing).forEach(([fromId, fEdges]) => {
    if (fEdges.length > 1) {
      const conds = new Set(fEdges.map((e) => extractBranchLabel(e.when)).filter(Boolean));
      if (conds.size > 1) forkPoints.add(fromId);
    }
  });

  // Branch colors
  const condColors = ["var(--green)", "var(--accent)", "var(--orange)", "var(--red)"];
  const condMarkers = ["url(#arrowG)", "url(#arrowB)", "url(#arrowO)", "url(#arrowR)"];
  const condColorMap: Record<string, number> = {};
  let condIdx = 0;

  // Build SVG
  let svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}" preserveAspectRatio="xMidYMid meet" style="width:100%;height:100%">
<defs>
  <filter id="glow"><feGaussianBlur stdDeviation="3" result="blur"/><feMerge><feMergeNode in="blur"/><feMergeNode in="SourceGraphic"/></feMerge></filter>
  <marker id="arrowG" markerWidth="7" markerHeight="5" refX="7" refY="2.5" orient="auto"><path d="M0,0 L7,2.5 L0,5" fill="var(--green)"/></marker>
  <marker id="arrowR" markerWidth="7" markerHeight="5" refX="7" refY="2.5" orient="auto"><path d="M0,0 L7,2.5 L0,5" fill="var(--red)"/></marker>
  <marker id="arrowB" markerWidth="7" markerHeight="5" refX="7" refY="2.5" orient="auto"><path d="M0,0 L7,2.5 L0,5" fill="var(--accent)"/></marker>
  <marker id="arrowO" markerWidth="7" markerHeight="5" refX="7" refY="2.5" orient="auto"><path d="M0,0 L7,2.5 L0,5" fill="var(--orange)"/></marker>
  <marker id="arrowD" markerWidth="7" markerHeight="5" refX="7" refY="2.5" orient="auto"><path d="M0,0 L7,2.5 L0,5" fill="var(--text-muted)"/></marker>
</defs>`;

  // --- Edges ---
  edgeList.forEach((e) => {
    const fp = nodePositions[e.from], tp = nodePositions[e.to];
    if (!fp || !tp) return;
    const fn = nodeMap[e.from], tn = nodeMap[e.to];

    const totalEmits = fn ? fn.emits.length : 1;
    const totalTakes = tn ? tn.takes.length : 1;
    const ep = emitPortPos(fp, 0, totalEmits);
    const tkp = takePortPos(tp, e.takeIdx, totalTakes);

    const x1 = ep.x, y1 = ep.y;
    const x2 = tkp.x, y2 = tkp.y;
    const dy = y2 - y1;
    const cy1 = y1 + dy * 0.35, cy2 = y2 - dy * 0.35;

    let color = "var(--text-muted)", marker = "url(#arrowD)";
    const cond = extractBranchLabel(e.when);
    if (cond) {
      if (!(cond in condColorMap)) {
        condColorMap[cond] = condIdx;
        condIdx = (condIdx + 1) % condColors.length;
      }
      const ci = condColorMap[cond];
      color = condColors[ci];
      marker = condMarkers[ci];
    } else if (e.when.includes("sync")) {
      color = "var(--accent)";
      marker = "url(#arrowB)";
    }

    const edgeInactive = inactive.has(e.to);
    const isActiveEdge = e.to === currentNodeId;
    let edgeCls = "edge-path";
    if (edgeInactive) edgeCls += " inactive-edge";
    else if (isActiveEdge) edgeCls += " active-edge";

    svg += `<path class="${edgeCls}" d="M${x1},${y1} C${x1},${cy1} ${x2},${cy2} ${x2},${y2}" stroke="${color}" marker-end="${marker}"/>`;

    // Branch condition label
    if (forkPoints.has(e.from) && cond) {
      const lx = x1 + (x2 - x1) * 0.45;
      const ly = y1 + dy * 0.3;
      const label = esc(cond);
      const tw = label.length * 5.5 + 8;
      const opac = edgeInactive ? 0.2 : 1;
      svg += `<rect class="edge-label-bg" x="${lx - tw / 2}" y="${ly - 8}" width="${tw}" height="14" rx="3" opacity="${opac}"/>`;
      svg += `<text class="edge-label" x="${lx}" y="${ly + 2}" text-anchor="middle" opacity="${opac}">${label}</text>`;
    }

    // Edge value annotations
    const fromIsSource = nodeMap[e.from]?.type === "source";
    const fromExecutedThisIter = iterationExecutedSet.has(e.from);
    const showValue = (fromIsSource || fromExecutedThisIter) && !edgeInactive;
    if (showValue && e.varName && currentBindings[e.varName] !== undefined) {
      const val = fmtVal(currentBindings[e.varName]);
      if (val) {
        const vx = (x1 + x2) / 2;
        const vy = y1 + (y2 - y1) * 0.55;
        const tw = val.length * 5 + 6;
        svg += `<rect class="edge-value-bg" x="${vx - tw / 2}" y="${vy - 7}" width="${tw}" height="12" fill="var(--bg)" opacity="0.85" rx="2"/>`;
        svg += `<text class="edge-value" x="${vx}" y="${vy + 2}" text-anchor="middle">${esc(val)}</text>`;
      }
    }
  });

  // --- Nodes + ports ---
  allIds.forEach((id) => {
    const p = nodePositions[id], n = nodeMap[id];
    if (!p || !n) return;

    const isActive = id === currentNodeId;
    const isSourceActive = activeSourceIds.has(id);
    const isExecuted = executedNodeSet.has(id);
    const isFailed = failedNodeSet.has(id);
    const isInactive = inactive.has(id);
    const stateClasses =
      (isActive ? " active" : "") +
      (isSourceActive && !isActive ? " source-active" : "") +
      (isExecuted ? " executed" : "") +
      (isFailed ? " failed" : "") +
      (isInactive ? " inactive" : "");

    if (n.type === "input") {
      svg += `<rect class="input-rect" x="${p.x}" y="${p.y}" width="${p.w}" height="${p.h}" fill="rgba(88,166,255,.1)" stroke="var(--accent)" stroke-width="1" rx="6"/>`;
      svg += `<text class="io-label" x="${p.x + p.w / 2}" y="${p.y + p.h / 2 + 4}" text-anchor="middle" fill="var(--text)" font-size="10">${esc(n.label)}</text>`;
    } else if (n.type === "output") {
      svg += `<rect class="output-rect" x="${p.x}" y="${p.y}" width="${p.w}" height="${p.h}" fill="rgba(63,185,80,.1)" stroke="var(--green)" stroke-width="1" rx="6"/>`;
      svg += `<text class="io-label" x="${p.x + p.w / 2}" y="${p.y + p.h / 2 + 4}" text-anchor="middle" fill="var(--text)" font-size="10">${esc(n.label)}</text>`;
    } else {
      const fillColor = n.type === "source"
        ? "rgba(88,166,255,.08)"
        : n.type === "sink"
        ? "rgba(63,185,80,.06)"
        : "var(--bg-tertiary)";
      const strokeColor = n.type === "source"
        ? "var(--accent)"
        : n.type === "sink"
        ? "var(--green)"
        : "var(--border)";

      svg += `<rect class="graph-node${stateClasses}" data-id="${esc(id)}" x="${p.x}" y="${p.y}" width="${p.w}" height="${p.h}" fill="${fillColor}" stroke="${strokeColor}" stroke-width="1.5" rx="8" style="cursor:pointer"/>`;
      svg += `<text class="node-label" x="${p.x + p.w / 2}" y="${p.y + 20}" text-anchor="middle" fill="var(--text)" font-size="11" style="pointer-events:none">${esc(n.label)}</text>`;
      if (n.sub) {
        svg += `<text class="node-sublabel" x="${p.x + p.w / 2}" y="${p.y + 34}" text-anchor="middle" fill="var(--text-muted)" font-size="9" style="pointer-events:none">${esc(n.sub)}</text>`;
      }

      // Take ports (top)
      n.takes.forEach((tName, ti) => {
        const tp = takePortPos(p, ti, n.takes.length);
        const connected = connectedTakes.has(id + ":" + tName);
        const portOpac = isInactive ? " opacity=\"0.2\"" : "";
        svg += `<circle cx="${tp.x}" cy="${tp.y}" r="${PORT_R}" fill="${connected ? "var(--text-muted)" : "var(--bg)"}" stroke="var(--text-muted)" stroke-width="1.5"${portOpac}/>`;
        svg += `<text x="${tp.x}" y="${tp.y - PORT_R - 2}" text-anchor="middle" fill="var(--text-muted)" font-size="8" style="pointer-events:none">${esc(tName)}</text>`;
      });

      // Emit ports (bottom)
      n.emits.forEach((eName, ei) => {
        const ep = emitPortPos(p, ei, n.emits.length);
        const connected = connectedEmits.has(id + ":" + eName);
        const portOpac = isInactive ? " opacity=\"0.2\"" : "";
        svg += `<circle cx="${ep.x}" cy="${ep.y}" r="${PORT_R}" fill="${connected ? "var(--text-muted)" : "var(--bg)"}" stroke="var(--text-muted)" stroke-width="1.5"${portOpac}/>`;
        svg += `<text x="${ep.x}" y="${ep.y + PORT_R + 10}" text-anchor="middle" fill="var(--text-muted)" font-size="8" style="pointer-events:none">${esc(eName)}</text>`;
      });

      // Breakpoint dot
      if (breakpoints.has(id)) {
        svg += `<circle class="bp-dot" cx="${p.x + 8}" cy="${p.y + 8}" r="5" fill="var(--red)" style="cursor:pointer" data-bp="${esc(id)}"/>`;
      }
    }
  });

  svg += "</svg>";
  container.innerHTML = svg;

  // Attach click handlers
  if (onNodeClick) {
    container.querySelectorAll(".graph-node").forEach((el) => {
      (el as SVGElement).addEventListener("click", () => {
        const nodeId = (el as SVGElement).dataset.id;
        if (nodeId) onNodeClick(nodeId);
      });
    });
    container.querySelectorAll(".bp-dot").forEach((el) => {
      (el as SVGElement).addEventListener("click", (ev) => {
        ev.stopPropagation();
        const nodeId = (el as SVGElement).dataset.bp;
        if (nodeId) onNodeClick(nodeId);
      });
    });
  }
}
