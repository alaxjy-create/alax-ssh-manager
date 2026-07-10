const writers = new Map<string, (data: string) => void>();
const transcripts = new Map<string, string>();
const MAX_TRANSCRIPT_LENGTH = 300_000;

export function registerTerminalWriter(sessionId: string, writer: (data: string) => void): void {
  writers.set(sessionId, writer);
}

export function unregisterTerminalWriter(sessionId: string): void {
  writers.delete(sessionId);
}

export function writeTerminalOutput(sessionId: string, data: string): void {
  appendTranscript(sessionId, data);
  const writer = writers.get(sessionId);
  if (writer) {
    writer(data);
  }
}

export function getBufferedOutput(sessionId: string): string {
  return transcripts.get(sessionId) ?? "";
}

export function clearTerminalBuffer(sessionId: string): void {
  transcripts.delete(sessionId);
}

function appendTranscript(sessionId: string, data: string): void {
  const next = `${transcripts.get(sessionId) ?? ""}${data}`;
  transcripts.set(sessionId, next.length > MAX_TRANSCRIPT_LENGTH ? next.slice(-MAX_TRANSCRIPT_LENGTH) : next);
}
