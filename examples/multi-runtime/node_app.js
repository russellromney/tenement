/** Minimal Node.js notes API with auth. */

const http = require("http");
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");

const PORT = parseInt(process.env.PORT || "8000");
const DATA_DIR = process.env.DATA_DIR || "./data";
const TENANT_ID = process.env.TENANT_ID || "unknown";
const NOTES_FILE = path.join(DATA_DIR, "notes.json");
const TOKEN_FILE = path.join(DATA_DIR, "token.txt");

function ensureDataDir() {
  fs.mkdirSync(DATA_DIR, { recursive: true });
}

function getToken() {
  ensureDataDir();
  if (fs.existsSync(TOKEN_FILE)) return fs.readFileSync(TOKEN_FILE, "utf8").trim();
  const token = crypto.createHash("sha256").update(`node-${TENANT_ID}`).digest("hex").slice(0, 32);
  fs.writeFileSync(TOKEN_FILE, token);
  return token;
}

function loadNotes() {
  if (!fs.existsSync(NOTES_FILE)) return [];
  return JSON.parse(fs.readFileSync(NOTES_FILE, "utf8"));
}

function saveNotes(notes) {
  ensureDataDir();
  fs.writeFileSync(NOTES_FILE, JSON.stringify(notes));
}

function checkAuth(headers) {
  const auth = headers.authorization || "";
  if (!auth) return { code: 401, body: { error: "Missing Authorization header" } };
  const parts = auth.split(" ");
  if (parts.length !== 2 || parts[0].toLowerCase() !== "bearer" || parts[1] !== getToken()) {
    return { code: 403, body: { error: "Invalid token" } };
  }
  return null;
}

function respond(res, code, body) {
  res.writeHead(code, { "Content-Type": "application/json" });
  res.end(JSON.stringify(body));
}

function readBody(req) {
  return new Promise((resolve) => {
    let data = "";
    req.on("data", (chunk) => (data += chunk));
    req.on("end", () => resolve(data ? JSON.parse(data) : {}));
  });
}

const server = http.createServer(async (req, res) => {
  if (req.method === "GET" && req.url === "/health") {
    respond(res, 200, { status: "ok", tenant: TENANT_ID, runtime: "node" });
  } else if (req.method === "GET" && req.url === "/token") {
    respond(res, 200, { tenant: TENANT_ID, token: getToken(), runtime: "node" });
  } else if (req.method === "GET" && req.url === "/notes") {
    const err = checkAuth(req.headers);
    if (err) return respond(res, err.code, err.body);
    respond(res, 200, { tenant: TENANT_ID, notes: loadNotes(), runtime: "node" });
  } else if (req.method === "POST" && req.url === "/notes") {
    const err = checkAuth(req.headers);
    if (err) return respond(res, err.code, err.body);
    const body = await readBody(req);
    const notes = loadNotes();
    const entry = { id: notes.length + 1, text: body.text };
    notes.push(entry);
    saveNotes(notes);
    respond(res, 201, { tenant: TENANT_ID, note: entry, runtime: "node" });
  } else {
    respond(res, 404, { error: "not found" });
  }
});

server.listen(PORT, "127.0.0.1", () => {
  console.log(`[node:${TENANT_ID}] listening on :${PORT}`);
});
