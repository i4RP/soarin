const express = require("express");
const { spawn } = require("child_process");
const http = require("http");

const app = express();
app.use(express.json({ limit: "10mb" }));

const BACKEND_URL = process.env.BACKEND_URL || "http://localhost:8080";
const SESSION_ID = process.env.SESSION_ID || "unknown";
const SLACK_CHANNEL = process.env.SLACK_CHANNEL || "";
const SLACK_THREAD_TS = process.env.SLACK_THREAD_TS || "";

let claudeProcess = null;
let isProcessing = false;

async function sendCallback(message) {
  try {
    const url = new URL("/api/worker/callback", BACKEND_URL);
    const body = JSON.stringify({
      session_id: SESSION_ID,
      channel_id: SLACK_CHANNEL,
      thread_ts: SLACK_THREAD_TS,
      message: message,
    });

    const resp = await fetch(url.toString(), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: body,
    });

    if (!resp.ok) {
      console.error(`Callback failed: ${resp.status}`);
    }
  } catch (err) {
    console.error(`Callback error: ${err.message}`);
  }
}

function runClaude(prompt) {
  return new Promise((resolve, reject) => {
    const args = [
      "--print",
      "--output-format",
      "text",
      "--max-turns",
      "25",
      prompt,
    ];

    console.log(`Running: claude ${args.join(" ")}`);

    const proc = spawn("claude", args, {
      cwd: "/home/worker/workspace",
      env: {
        ...process.env,
        HOME: "/home/worker",
      },
      stdio: ["pipe", "pipe", "pipe"],
    });

    claudeProcess = proc;
    let stdout = "";
    let stderr = "";

    proc.stdout.on("data", (data) => {
      const chunk = data.toString();
      stdout += chunk;
      console.log(`[claude stdout] ${chunk}`);
    });

    proc.stderr.on("data", (data) => {
      const chunk = data.toString();
      stderr += chunk;
      console.error(`[claude stderr] ${chunk}`);
    });

    proc.on("close", (code) => {
      claudeProcess = null;
      if (code === 0) {
        resolve(stdout.trim());
      } else {
        reject(new Error(`Claude exited with code ${code}: ${stderr}`));
      }
    });

    proc.on("error", (err) => {
      claudeProcess = null;
      reject(err);
    });
  });
}

app.get("/health", (req, res) => {
  res.json({
    status: "ok",
    session_id: SESSION_ID,
    is_processing: isProcessing,
  });
});

app.post("/prompt", async (req, res) => {
  const { prompt, session_id, user_id } = req.body;

  if (!prompt) {
    return res.status(400).json({ error: "prompt is required" });
  }

  if (isProcessing) {
    return res
      .status(429)
      .json({ error: "Already processing a prompt. Please wait." });
  }

  isProcessing = true;
  res.json({ status: "accepted", message: "Processing prompt..." });

  try {
    await sendCallback(":hourglass_flowing_sand: Processing your request...");

    const result = await runClaude(prompt);

    const maxLen = 3900;
    if (result.length > maxLen) {
      const chunks = [];
      for (let i = 0; i < result.length; i += maxLen) {
        chunks.push(result.substring(i, i + maxLen));
      }
      for (const chunk of chunks) {
        await sendCallback(chunk);
      }
    } else {
      await sendCallback(result || ":white_check_mark: Done (no output)");
    }
  } catch (err) {
    console.error(`Error running Claude: ${err.message}`);
    await sendCallback(`:x: Error: ${err.message}`);
  } finally {
    isProcessing = false;
  }
});

app.post("/cancel", (req, res) => {
  if (claudeProcess) {
    claudeProcess.kill("SIGTERM");
    claudeProcess = null;
    isProcessing = false;
    res.json({ status: "cancelled" });
  } else {
    res.json({ status: "no_process" });
  }
});

const PORT = process.env.PORT || 3000;
app.listen(PORT, "::", () => {
  console.log(`Worker bridge listening on port ${PORT}`);
  console.log(`Session: ${SESSION_ID}`);
  console.log(`Backend: ${BACKEND_URL}`);

  require("fs").mkdirSync("/home/worker/workspace", { recursive: true });
});
