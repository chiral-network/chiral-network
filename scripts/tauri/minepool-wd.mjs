import fs from 'fs';
import path from 'path';

const repoDir = '/home/samridh/chiral-network';
process.chdir(repoDir);

const driverUrl = process.env.TAURI_DRIVER_URL ?? 'http://127.0.0.1:4444';
const artifactsDir = path.join(repoDir, 'artifacts');
fs.mkdirSync(artifactsDir, { recursive: true });

const appPath = (() => {
  const envPath = process.env.TAURI_APP_PATH?.trim();
  if (envPath) return envPath;
  const debugPath = path.join(repoDir, 'src-tauri', 'target', 'debug', 'chiral-network');
  if (fs.existsSync(debugPath)) return debugPath;
  const releasePath = path.join(repoDir, 'src-tauri', 'target', 'release', 'chiral-network');
  if (fs.existsSync(releasePath)) return releasePath;
  throw new Error('missing tauri app binary set TAURI_APP_PATH');
})();

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const wdPost = async (url, body) => {
  const resp = await fetch(url, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
  const data = await resp.json();
  if (!resp.ok) {
    throw new Error(`webdriver error ${resp.status} ${JSON.stringify(data)}`);
  }
  return data.value ?? data;
};

const wdGet = async (url) => {
  const resp = await fetch(url);
  const data = await resp.json();
  if (!resp.ok) {
    throw new Error(`webdriver error ${resp.status} ${JSON.stringify(data)}`);
  }
  return data.value ?? data;
};

const elemId = (value) => {
  if (!value) return null;
  if (typeof value === 'string') return value;
  return value['element-6066-11e4-a52e-4f735466cecf'] ?? value.ELEMENT ?? null;
};

const findCss = async (sid, selector) => {
  try {
    const value = await wdPost(`${driverUrl}/session/${sid}/element`, {
      using: 'css selector',
      value: selector,
    });
    return elemId(value);
  } catch (err) {
    if (String(err).includes('no such element')) return null;
    throw err;
  }
};

const findXpath = async (sid, selector) => {
  try {
    const value = await wdPost(`${driverUrl}/session/${sid}/element`, {
      using: 'xpath',
      value: selector,
    });
    return elemId(value);
  } catch (err) {
    if (String(err).includes('no such element')) return null;
    throw err;
  }
};

const clickElem = async (sid, id) => {
  await wdPost(`${driverUrl}/session/${sid}/element/${id}/click`, {});
};

const sendKeys = async (sid, id, text) => {
  await wdPost(`${driverUrl}/session/${sid}/element/${id}/value`, {
    text,
    value: text.split(''),
  });
};

const getText = async (sid, id) => {
  return await wdGet(`${driverUrl}/session/${sid}/element/${id}/text`);
};

const screenshot = async (sid, name) => {
  const b64 = await wdGet(`${driverUrl}/session/${sid}/screenshot`);
  const buf = Buffer.from(b64, 'base64');
  const outPath = path.join(artifactsDir, name);
  fs.writeFileSync(outPath, buf);
};

const waitFor = async (fn, timeoutMs, stepMs) => {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    const res = await fn();
    if (res) return res;
    await sleep(stepMs);
  }
  throw new Error('timeout waiting for condition');
};

const waitForText = async (sid, id, expected, timeoutMs) => {
  await waitFor(async () => {
    const text = await getText(sid, id);
    return text.trim() === expected ? text : null;
  }, timeoutMs, 500);
};

const waitForContains = async (sid, id, needle, timeoutMs) => {
  await waitFor(async () => {
    const text = await getText(sid, id);
    return text.includes(needle) ? text : null;
  }, timeoutMs, 500);
};

const poolName = `Test Pool ${Date.now()}`;
const poolUrl = (() => {
  const envUrl = process.env.CHIRAL_POOL_URL?.trim();
  if (envUrl) return envUrl;
  const driver = new URL(driverUrl);
  return `stratum+tcp://${driver.hostname}:${driver.port || '4444'}`;
})();

const caps = {
  capabilities: {
    alwaysMatch: {
      platformName: 'linux',
      'tauri:options': {
        application: appPath,
      },
    },
  },
};

let sid;
try {
  const session = await wdPost(`${driverUrl}/session`, caps);
  sid = session.sessionId ?? session.value?.sessionId ?? session.id;
  if (!sid) {
    throw new Error('missing session id from webdriver');
  }

  const navAccount = await waitFor(() => findCss(sid, 'a[href="/account"]'), 120000, 500);
  await clickElem(sid, navAccount);

  const header = await waitFor(() => findCss(sid, 'h1'), 120000, 500);
  await waitForText(sid, header, 'Account', 120000);

  const createBtn = await waitFor(() => findXpath(sid, "//button[contains(., 'Create New Account')]"), 120000, 500);
  await clickElem(sid, createBtn);

  await screenshot(sid, 'account-created.png');

  const navMining = await waitFor(() => findCss(sid, 'a[href="/mining"]'), 120000, 500);
  await clickElem(sid, navMining);

  const miningHeader = await waitFor(() => findCss(sid, 'h1'), 120000, 500);
  await waitForText(sid, miningHeader, 'Mining', 120000);

  const createPoolBtn = await waitFor(() => findXpath(sid, "//button[contains(., 'Create Pool')]"), 120000, 500);
  await clickElem(sid, createPoolBtn);

  const poolNameInput = await waitFor(() => findCss(sid, 'input#pool-name'), 120000, 500);
  const poolUrlInput = await waitFor(() => findCss(sid, 'input#pool-url'), 120000, 500);

  await sendKeys(sid, poolNameInput, poolName);
  await sendKeys(sid, poolUrlInput, poolUrl);

  await screenshot(sid, 'pool-create-form.png');

  const createPoolSubmit = await waitFor(() => findXpath(sid, "//button[contains(., 'Create Pool')]"), 120000, 500);
  await clickElem(sid, createPoolSubmit);

  const connected = await waitFor(() => findXpath(sid, "//h4[contains(., 'Connected to')]"), 120000, 500);
  await waitForContains(sid, connected, poolName, 120000);

  await screenshot(sid, 'pool-connected.png');

  console.log(`ok pool created and connected to ${poolName} (${poolUrl})`);
  console.log(`screenshots in ${artifactsDir}`);
} finally {
  if (sid) {
    await fetch(`${driverUrl}/session/${sid}`, { method: 'DELETE' }).catch(() => {});
  }
}
