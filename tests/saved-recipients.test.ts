// Tests for saved recipients feature in Account.svelte
// Tests localStorage persistence, CRUD operations, duplicate detection,
// lastUsed tracking, and recipient label integration.

import { describe, it, expect, beforeEach, vi } from "vitest";

// Mock localStorage
const localStorageMock = (() => {
  let store: Record<string, string> = {};
  return {
    getItem: (key: string) => store[key] || null,
    setItem: (key: string, value: string) => {
      store[key] = value;
    },
    removeItem: (key: string) => {
      delete store[key];
    },
    clear: () => {
      store = {};
    },
    get length() {
      return Object.keys(store).length;
    },
    key: (i: number) => Object.keys(store)[i] || null,
  };
})();

global.localStorage = localStorageMock as any;

// Types matching Account.svelte implementation
interface SavedRecipient {
  id: string;
  label: string;
  address: string;
  lastUsed: number;
}

const SAVED_RECIPIENTS_KEY = "chiral_saved_recipients";

// Helper functions that mirror Account.svelte logic
function loadSavedRecipients(): SavedRecipient[] {
  try {
    const stored = localStorage.getItem(SAVED_RECIPIENTS_KEY);
    if (stored) {
      return JSON.parse(stored);
    }
  } catch {
    // ignore
  }
  return [];
}

function saveSavedRecipients(recipients: SavedRecipient[]) {
  localStorage.setItem(SAVED_RECIPIENTS_KEY, JSON.stringify(recipients));
}

function addRecipient(
  recipients: SavedRecipient[],
  label: string,
  address: string
): { recipients: SavedRecipient[]; error?: string } {
  if (!label.trim() || !address) {
    return { recipients, error: "Label and address required" };
  }
  if (!address.startsWith("0x") || address.length !== 42) {
    return { recipients, error: "Invalid address" };
  }
  if (
    recipients.some((r) => r.address.toLowerCase() === address.toLowerCase())
  ) {
    return { recipients, error: "Already saved" };
  }
  const newRecipient: SavedRecipient = {
    id: `r-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    label: label.trim(),
    address,
    lastUsed: Date.now(),
  };
  return { recipients: [...recipients, newRecipient] };
}

function deleteRecipient(
  recipients: SavedRecipient[],
  id: string
): SavedRecipient[] {
  return recipients.filter((r) => r.id !== id);
}

function getRecipientLabel(
  recipients: SavedRecipient[],
  address: string
): string | null {
  const r = recipients.find(
    (r) => r.address.toLowerCase() === address.toLowerCase()
  );
  return r ? r.label : null;
}

describe("Saved Recipients", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  // =========================================================================
  // localStorage persistence
  // =========================================================================

  describe("localStorage Persistence", () => {
    it("should load empty array when no saved data exists", () => {
      const recipients = loadSavedRecipients();
      expect(recipients).toEqual([]);
    });

    it("should save and load recipients from localStorage", () => {
      const recipients: SavedRecipient[] = [
        {
          id: "r-1",
          label: "Alice",
          address: "0x1234567890abcdef1234567890abcdef12345678",
          lastUsed: 1000,
        },
        {
          id: "r-2",
          label: "Bob",
          address: "0xabcdefabcdefabcdefabcdefabcdefabcdefabcd",
          lastUsed: 2000,
        },
      ];

      saveSavedRecipients(recipients);
      const loaded = loadSavedRecipients();

      expect(loaded).toHaveLength(2);
      expect(loaded[0].label).toBe("Alice");
      expect(loaded[1].label).toBe("Bob");
      expect(loaded[0].address).toBe(
        "0x1234567890abcdef1234567890abcdef12345678"
      );
    });

    it("should handle corrupted localStorage data gracefully", () => {
      localStorage.setItem(SAVED_RECIPIENTS_KEY, "not valid json{{{");
      const recipients = loadSavedRecipients();
      expect(recipients).toEqual([]);
    });

    it("should persist across multiple save/load cycles", () => {
      const r1: SavedRecipient = {
        id: "r-1",
        label: "Charlie",
        address: "0x1111111111111111111111111111111111111111",
        lastUsed: 1000,
      };

      saveSavedRecipients([r1]);
      let loaded = loadSavedRecipients();
      expect(loaded).toHaveLength(1);

      const r2: SavedRecipient = {
        id: "r-2",
        label: "Dave",
        address: "0x2222222222222222222222222222222222222222",
        lastUsed: 2000,
      };
      saveSavedRecipients([...loaded, r2]);
      loaded = loadSavedRecipients();
      expect(loaded).toHaveLength(2);
    });

    it("should handle empty array save/load", () => {
      saveSavedRecipients([]);
      const loaded = loadSavedRecipients();
      expect(loaded).toEqual([]);
    });
  });

  // =========================================================================
  // Adding recipients
  // =========================================================================

  describe("Adding Recipients", () => {
    it("should add a new recipient with valid address", () => {
      const result = addRecipient(
        [],
        "Alice",
        "0x1234567890abcdef1234567890abcdef12345678"
      );
      expect(result.error).toBeUndefined();
      expect(result.recipients).toHaveLength(1);
      expect(result.recipients[0].label).toBe("Alice");
      expect(result.recipients[0].address).toBe(
        "0x1234567890abcdef1234567890abcdef12345678"
      );
    });

    it("should reject empty label", () => {
      const result = addRecipient(
        [],
        "",
        "0x1234567890abcdef1234567890abcdef12345678"
      );
      expect(result.error).toBe("Label and address required");
      expect(result.recipients).toHaveLength(0);
    });

    it("should reject whitespace-only label", () => {
      const result = addRecipient(
        [],
        "   ",
        "0x1234567890abcdef1234567890abcdef12345678"
      );
      expect(result.error).toBe("Label and address required");
    });

    it("should reject invalid address (not 0x prefix)", () => {
      const result = addRecipient([], "Alice", "1234567890abcdef1234567890abcdef12345678ab");
      expect(result.error).toBe("Invalid address");
    });

    it("should reject address with wrong length", () => {
      const result = addRecipient([], "Alice", "0x1234");
      expect(result.error).toBe("Invalid address");
    });

    it("should reject empty address", () => {
      const result = addRecipient([], "Alice", "");
      expect(result.error).toBe("Label and address required");
    });

    it("should trim label whitespace", () => {
      const result = addRecipient(
        [],
        "  Alice  ",
        "0x1234567890abcdef1234567890abcdef12345678"
      );
      expect(result.recipients[0].label).toBe("Alice");
    });

    it("should assign unique id starting with 'r-'", () => {
      const result = addRecipient(
        [],
        "Alice",
        "0x1234567890abcdef1234567890abcdef12345678"
      );
      expect(result.recipients[0].id).toMatch(/^r-\d+-[a-z0-9]+$/);
    });

    it("should set lastUsed to current timestamp", () => {
      const before = Date.now();
      const result = addRecipient(
        [],
        "Alice",
        "0x1234567890abcdef1234567890abcdef12345678"
      );
      const after = Date.now();
      expect(result.recipients[0].lastUsed).toBeGreaterThanOrEqual(before);
      expect(result.recipients[0].lastUsed).toBeLessThanOrEqual(after);
    });

    it("should add multiple recipients", () => {
      let recipients: SavedRecipient[] = [];
      const addresses = [
        "0x1111111111111111111111111111111111111111",
        "0x2222222222222222222222222222222222222222",
        "0x3333333333333333333333333333333333333333",
      ];

      for (let i = 0; i < addresses.length; i++) {
        const result = addRecipient(
          recipients,
          `Person ${i}`,
          addresses[i]
        );
        recipients = result.recipients;
      }

      expect(recipients).toHaveLength(3);
    });
  });

  // =========================================================================
  // Duplicate detection
  // =========================================================================

  describe("Duplicate Detection", () => {
    const existing: SavedRecipient[] = [
      {
        id: "r-1",
        label: "Alice",
        address: "0x1234567890abcdef1234567890abcdef12345678",
        lastUsed: 1000,
      },
    ];

    it("should reject duplicate address (exact match)", () => {
      const result = addRecipient(
        existing,
        "Alice Again",
        "0x1234567890abcdef1234567890abcdef12345678"
      );
      expect(result.error).toBe("Already saved");
      expect(result.recipients).toHaveLength(1); // unchanged
    });

    it("should reject duplicate address (case insensitive)", () => {
      const result = addRecipient(
        existing,
        "ALICE",
        "0x1234567890ABCDEF1234567890ABCDEF12345678"
      );
      expect(result.error).toBe("Already saved");
    });

    it("should allow different addresses", () => {
      const result = addRecipient(
        existing,
        "Bob",
        "0xabcdefabcdefabcdefabcdefabcdefabcdefabcd"
      );
      expect(result.error).toBeUndefined();
      expect(result.recipients).toHaveLength(2);
    });
  });

  // =========================================================================
  // Deleting recipients
  // =========================================================================

  describe("Deleting Recipients", () => {
    it("should delete recipient by id", () => {
      const recipients: SavedRecipient[] = [
        {
          id: "r-1",
          label: "Alice",
          address: "0x1111111111111111111111111111111111111111",
          lastUsed: 1000,
        },
        {
          id: "r-2",
          label: "Bob",
          address: "0x2222222222222222222222222222222222222222",
          lastUsed: 2000,
        },
      ];

      const result = deleteRecipient(recipients, "r-1");
      expect(result).toHaveLength(1);
      expect(result[0].label).toBe("Bob");
    });

    it("should handle deleting non-existent id", () => {
      const recipients: SavedRecipient[] = [
        {
          id: "r-1",
          label: "Alice",
          address: "0x1111111111111111111111111111111111111111",
          lastUsed: 1000,
        },
      ];

      const result = deleteRecipient(recipients, "r-999");
      expect(result).toHaveLength(1); // unchanged
    });

    it("should handle deleting from empty list", () => {
      const result = deleteRecipient([], "r-1");
      expect(result).toHaveLength(0);
    });

    it("should delete all recipients one by one", () => {
      let recipients: SavedRecipient[] = [
        {
          id: "r-1",
          label: "A",
          address: "0x1111111111111111111111111111111111111111",
          lastUsed: 1000,
        },
        {
          id: "r-2",
          label: "B",
          address: "0x2222222222222222222222222222222222222222",
          lastUsed: 2000,
        },
        {
          id: "r-3",
          label: "C",
          address: "0x3333333333333333333333333333333333333333",
          lastUsed: 3000,
        },
      ];

      recipients = deleteRecipient(recipients, "r-2");
      expect(recipients).toHaveLength(2);
      recipients = deleteRecipient(recipients, "r-1");
      expect(recipients).toHaveLength(1);
      recipients = deleteRecipient(recipients, "r-3");
      expect(recipients).toHaveLength(0);
    });
  });

  // =========================================================================
  // Recipient label lookup
  // =========================================================================

  describe("Recipient Label Lookup", () => {
    const recipients: SavedRecipient[] = [
      {
        id: "r-1",
        label: "Alice",
        address: "0x1234567890abcdef1234567890abcdef12345678",
        lastUsed: 1000,
      },
      {
        id: "r-2",
        label: "Bob",
        address: "0xabcdefabcdefabcdefabcdefabcdefabcdefabcd",
        lastUsed: 2000,
      },
    ];

    it("should return label for known address", () => {
      const label = getRecipientLabel(
        recipients,
        "0x1234567890abcdef1234567890abcdef12345678"
      );
      expect(label).toBe("Alice");
    });

    it("should return label case-insensitively", () => {
      const label = getRecipientLabel(
        recipients,
        "0x1234567890ABCDEF1234567890ABCDEF12345678"
      );
      expect(label).toBe("Alice");
    });

    it("should return null for unknown address", () => {
      const label = getRecipientLabel(
        recipients,
        "0x0000000000000000000000000000000000000000"
      );
      expect(label).toBeNull();
    });

    it("should return null for empty list", () => {
      const label = getRecipientLabel(
        [],
        "0x1234567890abcdef1234567890abcdef12345678"
      );
      expect(label).toBeNull();
    });
  });

  // =========================================================================
  // lastUsed sorting
  // =========================================================================

  describe("Sorting by lastUsed", () => {
    it("should sort recipients by most recently used first", () => {
      const recipients: SavedRecipient[] = [
        {
          id: "r-1",
          label: "Old",
          address: "0x1111111111111111111111111111111111111111",
          lastUsed: 1000,
        },
        {
          id: "r-2",
          label: "Recent",
          address: "0x2222222222222222222222222222222222222222",
          lastUsed: 3000,
        },
        {
          id: "r-3",
          label: "Middle",
          address: "0x3333333333333333333333333333333333333333",
          lastUsed: 2000,
        },
      ];

      const sorted = [...recipients].sort((a, b) => b.lastUsed - a.lastUsed);
      expect(sorted[0].label).toBe("Recent");
      expect(sorted[1].label).toBe("Middle");
      expect(sorted[2].label).toBe("Old");
    });

    it("should update lastUsed when recipient is used for sending", () => {
      const recipients: SavedRecipient[] = [
        {
          id: "r-1",
          label: "Alice",
          address: "0x1234567890abcdef1234567890abcdef12345678",
          lastUsed: 1000,
        },
      ];

      // Simulate updating lastUsed on send
      const idx = recipients.findIndex(
        (r) =>
          r.address.toLowerCase() ===
          "0x1234567890abcdef1234567890abcdef12345678"
      );
      if (idx !== -1) {
        recipients[idx] = { ...recipients[idx], lastUsed: Date.now() };
      }

      expect(recipients[0].lastUsed).toBeGreaterThan(1000);
    });
  });

  // =========================================================================
  // Full lifecycle integration
  // =========================================================================

  describe("Full Lifecycle", () => {
    it("should support add, save, load, use, delete cycle", () => {
      // 1. Add recipients
      let recipients: SavedRecipient[] = [];
      let result = addRecipient(
        recipients,
        "Alice",
        "0x1234567890abcdef1234567890abcdef12345678"
      );
      recipients = result.recipients;

      result = addRecipient(
        recipients,
        "Bob",
        "0xabcdefabcdefabcdefabcdefabcdefabcdefabcd"
      );
      recipients = result.recipients;
      expect(recipients).toHaveLength(2);

      // 2. Save to localStorage
      saveSavedRecipients(recipients);

      // 3. Load from localStorage (simulating page reload)
      const loaded = loadSavedRecipients();
      expect(loaded).toHaveLength(2);

      // 4. Look up label for transaction
      const label = getRecipientLabel(
        loaded,
        "0x1234567890abcdef1234567890abcdef12345678"
      );
      expect(label).toBe("Alice");

      // 5. Delete one recipient
      const afterDelete = deleteRecipient(loaded, loaded[0].id);
      expect(afterDelete).toHaveLength(1);

      // 6. Save and reload
      saveSavedRecipients(afterDelete);
      const reloaded = loadSavedRecipients();
      expect(reloaded).toHaveLength(1);
      expect(reloaded[0].label).toBe("Bob");
    });

    it("should handle 50 recipients without issues", () => {
      let recipients: SavedRecipient[] = [];
      for (let i = 0; i < 50; i++) {
        const addr = `0x${i.toString(16).padStart(40, "0")}`;
        const result = addRecipient(recipients, `User ${i}`, addr);
        recipients = result.recipients;
      }
      expect(recipients).toHaveLength(50);

      saveSavedRecipients(recipients);
      const loaded = loadSavedRecipients();
      expect(loaded).toHaveLength(50);
    });
  });
});
