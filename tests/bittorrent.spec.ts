import { test, expect, _electron as electron } from '@playwright/test';
import * as fs from 'fs';
import * as path from 'path';

// Global settings for the tests
const LAUNCH_TIMEOUT = 60000; // 60 seconds to launch the app
const ACTION_TIMEOUT = 30000; // 30 seconds for actions like clicks and navigation
const SEED_FILE_PATH = path.join(__dirname, 'test-seed-file.txt');
const DOWNLOAD_DIR = path.join(__dirname, 'test-downloads');

let magnetLink = '';

test.describe.configure({ mode: 'serial' });

/**
 * E2E Test for Seeding a File
 *
 * This test simulates a user seeding a file through the UI.
 * 1. Creates a dummy file to be seeded.
 * 2. Launches the Chiral Network application.
 * 3. Navigates to the "Torrents" page.
 * 4. Uses the file chooser to select the dummy file for seeding.
 * 5. Clicks the "Seed File" button.
 * 6. Captures the generated magnet link for the download test.
 * 7. Verifies that the UI confirms the seeding has started.
 */
test('should allow a user to seed a file and get a magnet link', async () => {
  // 1. Create a dummy file for seeding
  if (!fs.existsSync(SEED_FILE_PATH)) {
    fs.writeFileSync(SEED_FILE_PATH, 'This is a test file for Chiral Network E2E testing.');
  }

  // 2. Launch the application
  const electronApp = await electron.launch({
    args: ['.'],
    timeout: LAUNCH_TIMEOUT,
  });
  const window = await electronApp.firstWindow();
  await window.waitForLoadState('domcontentloaded');

  // 3. Navigate to the Torrents page
  await window.click('a[href="/torrents"]', { timeout: ACTION_TIMEOUT });
  await expect(window.locator('h1')).toHaveText('Torrents');

  // 4. Set the file for the file input
  const fileChooserPromise = window.waitForEvent('filechooser');
  await window.click('button:has-text("Seed File")');
  const fileChooser = await fileChooserPromise;
  await fileChooser.setFiles(SEED_FILE_PATH);

  // 5. The file is now selected. The same button might now trigger the seeding process,
  // or a new button might appear. Assuming the same button is used.
  // This step depends on the exact UI flow.
  // Let's assume a toast notification appears with the magnet link.

  // 6. Wait for the magnet link to be generated and displayed
  const magnetElement = window.locator('input[value^="magnet:?xt=urn:btih:"]');
  await magnetElement.waitFor({ state: 'visible', timeout: ACTION_TIMEOUT });

  const generatedLink = await magnetElement.inputValue();
  expect(generatedLink).toContain('magnet:?xt=urn:btih:');
  magnetLink = generatedLink; // Save for the next test
  console.log(`Generated Magnet Link: ${magnetLink}`);

  // 7. Verify seeding status in UI
  await expect(window.locator('div:text("Successfully started seeding")')).toBeVisible();

  // Keep the seeder instance running for the download test
  // In a real CI setup, you would manage this process carefully.
  // For local testing, we'll just let it run and close it in the next test.
  // electronApp.close();
});

/**
 * E2E Test for Downloading a Torrent
 *
 * This test simulates a user downloading a file using a magnet link.
 * It depends on the previous test to have a running seeder.
 * 1. Launches a new instance of the application.
 * 2. Navigates to the "Torrents" page.
 * 3. Pastes the magnet link from the seeding test into the input field.
 * 4. Clicks the "Download" button.
 * 5. Verifies that the download appears in the "Active Transfers" list.
 * 6. Monitors the UI for progress and waits for completion.
 * 7. Verifies the downloaded file exists and has the correct content.
 */
test('should allow a user to download a file using a magnet link', async () => {
  test.skip(!magnetLink, 'Seeding test must run first to generate a magnet link.');

  // Ensure download directory exists
  if (!fs.existsSync(DOWNLOAD_DIR)) {
    fs.mkdirSync(DOWNLOAD_DIR, { recursive: true });
  }

  // 1. Launch a new application instance for the downloader
  const electronApp = await electron.launch({
    args: ['.'],
    timeout: LAUNCH_TIMEOUT,
    env: { ...process.env, CHIRAL_INSTANCE_ID: '2' } // Use a different instance ID
  });
  const window = await electronApp.firstWindow();
  await window.waitForLoadState('domcontentloaded');

  // 2. Navigate to the Torrents page
  await window.click('a[href="/torrents"]', { timeout: ACTION_TIMEOUT });

  // 3. Paste the magnet link
  await window.fill('input[placeholder="magnet:?xt=urn:btih:..."]', magnetLink);

  // 4. Click Download
  await window.click('button:has-text("Download")');

  // 5. Verify the download appears in the active transfers list
  const transferItem = window.locator('.transfer-item:has-text("test-seed-file.txt")');
  await expect(transferItem).toBeVisible({ timeout: ACTION_TIMEOUT });

  // 6. Wait for the download to complete
  // This is indicated by the progress bar reaching 100% or a "Completed" status.
  await expect(transferItem.locator('.progress-bar')).toHaveAttribute('style', 'width: 100%;', { timeout: 120000 }); // 2-minute timeout for download
  await expect(transferItem.locator(':text("Completed")')).toBeVisible();

  // 7. Verify the downloaded file
  const downloadedFilePath = path.join(DOWNLOAD_DIR, 'test-seed-file.txt');
  expect(fs.existsSync(downloadedFilePath)).toBe(true);
  const content = fs.readFileSync(downloadedFilePath, 'utf-8');
  expect(content).toBe('This is a test file for Chiral Network E2E testing.');

  await electronApp.close();
});
