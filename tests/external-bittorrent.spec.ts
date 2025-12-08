import { test, expect, _electron as electron } from '@playwright/test';
import * as path from 'path';

// Global settings for the tests
const LAUNCH_TIMEOUT = 120000; // 120 seconds to launch the app
const ACTION_TIMEOUT = 30000; // 30 seconds for actions like clicks and navigation

// A magnet link for a well-known public domain torrent (Big Buck Bunny)
const EXTERNAL_MAGNET_LINK = 'magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn=Big%20Buck%20Bunny';

test.describe.configure({ mode: 'serial' });

/**
 * E2E Test for Downloading a Torrent from an External Magnet Link
 *
 * This test simulates a user downloading a file from a magnet link for a file
 * that is not present in the Chiral Network's DHT.
 *
 * 1. Launches the Chiral Network application.
 * 2. Navigates to the "Download" page.
 * 3. Selects the 'magnet' search mode.
 * 4. Pastes the external magnet link into the input field.
 * 5. Clicks the "Search" button, which should prepare the download.
 * 6. Clicks the "Download" button on the search result card.
 * 7. Confirms the download in the peer selection modal.
 * 8. Verifies that the download appears in the list of active downloads.
 * 9. Verifies that the download is progressing and not in a failed state.
 */
test('should allow a user to download a file from an external magnet link', async ({ page }) => {
  test.setTimeout(300000); // Set a longer timeout for this specific test (5 minutes)

  // 1. Launch the application
  const electronApp = await electron.launch({
    args: ['.'],
    timeout: LAUNCH_TIMEOUT,
  });

  // Capture console messages from the main process
  electronApp.on('console', msg => {
    console.log(`[Electron Main]: ${msg.text()}`);
  });

  const window = await electronApp.firstWindow({ timeout: LAUNCH_TIMEOUT });

  // Capture console messages from the renderer process
  window.on('console', msg => {
    console.log(`[Electron Renderer]: ${msg.text()}`);
  });

  await window.waitForLoadState('domcontentloaded');

  // 2. Navigate to the Download page (assuming it's the root page or accessible)
  await expect(window.locator('h1:has-text("Dashboard")')).toBeVisible();

  // Find the download search section
  const downloadSearchSection = window.locator('div.space-y-4:has(label:has-text("Add New Download"))');
  await expect(downloadSearchSection).toBeVisible();

  // 3. Select the 'magnet' search mode
  await downloadSearchSection.locator('select').selectOption('magnet');

  // 4. Paste the external magnet link
  await downloadSearchSection.locator('input[placeholder="magnet:?xt=urn:btih:..."]').fill(EXTERNAL_MAGNET_LINK);

  // 5. Click the "Search" button
  await downloadSearchSection.locator('button:has-text("Search")').click();

  // 6. Wait for the search result card to appear and click "Download"
  const searchResultCard = window.locator('div.pt-6:has-text("Magnet Link Download")');
  await expect(searchResultCard).toBeVisible({ timeout: ACTION_TIMEOUT });
  await searchResultCard.locator('button:has-text("Download")').click();

  // 7. The peer selection modal should appear. Confirm the download.
  const peerSelectionModal = window.locator('div[role="dialog"]:has-text("Confirm Download")');
  await expect(peerSelectionModal).toBeVisible({ timeout: ACTION_TIMEOUT });
  await peerSelectionModal.locator('button:has-text("Confirm")').click();

  // 8. Verify that the download appears in the active downloads list.
  await window.click('a[href="/downloads"]');
  const downloadsList = window.locator('div.overflow-y-auto.pb-20');
  const downloadItem = downloadsList.locator('div.grid.grid-cols-12:has-text("Big Buck Bunny")');
  await expect(downloadItem).toBeVisible({ timeout: ACTION_TIMEOUT });

  // 9. Verifies that the download is progressing and not in a failed state.
  // Expect the status not to be "Failed" and for some progress to eventually show up.
  await expect(downloadItem).not.toContainText('Failed', { timeout: 60000 }); // Max 60s to not see "Failed"

  const progressBar = downloadItem.locator('div[role="progressbar"]');
  await expect(progressBar).toBeVisible();
  
  // Wait for the progress bar to show some progress (width > 0)
  await expect(progressBar).toHaveAttribute('style', /width:\s*(\d+\.?\d*[1-9]\d*|100)%;/, { timeout: 60000 });

  // Wait for the download to be completed or seeding
  await expect(downloadItem).toMatch(/Completed|Seeding/, { timeout: 180000 });


  await electronApp.close();
});