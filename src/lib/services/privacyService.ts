// Service for managing privacy and anonymous routing settings

export class PrivacyService {
  private anonymousMode = false;
  private multiHopEnabled = false;

  setAnonymousMode(enabled: boolean) {
    this.anonymousMode = enabled;
  }

  isAnonymousMode(): boolean {
    return this.anonymousMode;
  }

  setMultiHop(enabled: boolean) {
    this.multiHopEnabled = enabled;
  }

  isMultiHopEnabled(): boolean {
    return this.multiHopEnabled;
  }

  // Advanced: allow user to set a custom privacy profile
  setPrivacyProfile(profile: { anonymous: boolean; multiHop: boolean }) {
    this.anonymousMode = profile.anonymous;
    this.multiHopEnabled = profile.multiHop;
  }

  // Apply privacy settings to a network request (no-op for now)
  applyPrivacySettings(request: any) {
    return request;
  }
}

export const privacyService = new PrivacyService();
