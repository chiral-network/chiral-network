#!/usr/bin/env node

/**
 * Git Contribution Analyzer
 * Analyzes git history and generates contribution statistics by team and individual
 */

const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');

// Team prefixes to identify team membership
const TEAM_PREFIXES = [
  'totoro', 'eagles', 'iris', 'walrus', 'pandas', 'potato',
  'turtle', 'orca', 'hawks', 'cactus', 'whales'
];

// Author name/email mappings to normalize contributors
// Maps various author identities to a canonical (team, member) tuple
const AUTHOR_MAPPINGS = {
  // Instructors/Maintainers (team: "staff")
  'Steven Tung': { team: 'staff', member: 'steven-tung' },
  'chiral-steven-tung-2': { team: 'staff', member: 'steven-tung' },
  'Shuai Mu': { team: 'staff', member: 'shuai-mu' },
  'shuaimu': { team: 'staff', member: 'shuai-mu' },
  'chiral-Ze-Tang': { team: 'staff', member: 'ze-tang' },

  // Bots/External (team: "external")
  'copilot-swe-agent[bot]': { team: 'external', member: 'copilot-bot' },
  'Claude': { team: 'external', member: 'claude-ai' },
  'Chiral Dev': { team: 'external', member: 'chiral-dev' },

  // Manual mappings for authors without team prefix
  'Jake Mallen': { team: 'turtle', member: 'jake-mallen' },
  'Zeynep': { team: 'iris', member: 'zeynep-tasoglu' },
  'Zeynep Tasoglu': { team: 'iris', member: 'zeynep-tasoglu' },
  'ZeynepST': { team: 'iris', member: 'zeynep-tasoglu' },
  'Fahim Jawad': { team: 'iris', member: 'fahim-jawad' },
  'Sharanya Kataru': { team: 'iris', member: 'sharanya-kataru' },
  'Qianhe Zhu': { team: 'pandas', member: 'sara-zhu' },
  'Ashley Wu': { team: 'pandas', member: 'ashley-wu' },
  'Yvette Han': { team: 'pandas', member: 'yvette-han' },
  'Yvette-Han': { team: 'pandas', member: 'yvette-han' },
  'ngitman': { team: 'potato', member: 'nicholas-gitman' },
  'Nicholas Gitman': { team: 'potato', member: 'nicholas-gitman' },
  'Belle Zheng': { team: 'orca', member: 'belle-zheng' },
  'LeeEisenberg': { team: 'orca', member: 'lee-eisenberg' },
  'Michael Tso': { team: 'walrus', member: 'michael-tso' },
  'Aaron Purnawan': { team: 'totoro', member: 'aaron-purnawan' },
  'Angela Lee': { team: 'totoro', member: 'angela-lee' },
  'leeangela9': { team: 'totoro', member: 'angela-lee' },
  'Amy Z': { team: 'orca', member: 'amy-zhang' },
  'Amyz04': { team: 'orca', member: 'amy-zhang' },
  'CYB101NAME': { team: 'iris', member: 'zeynep-tasoglu' }, // Appears to be Zeynep based on commits
  'GMascetti04': { team: 'potato', member: 'giovanni-mascetti' },
  'Giovanni Mascetti': { team: 'potato', member: 'giovanni-mascetti' },
  'Wei Jie Li': { team: 'potato', member: 'weijie-li' },
  'weijieli1': { team: 'potato', member: 'weijie-li' },
  'khan shalti': { team: 'potato', member: 'khannah-shaltiel' },
  'Gilho Kim': { team: 'eagles', member: 'gilho-kim' },
  'David': { team: 'turtle', member: 'david-lai' },
  'ninadNawrikar': { team: 'orca', member: 'ninad-nawrikar' },
  'Eliot Shytaj': { team: 'hawks', member: 'eliot-shytaj' },
  'Samuel Buena': { team: 'hawks', member: 'samuel-buena' },
  'Ashish Jalwan': { team: 'hawks', member: 'ashish-jalwan' },
  'Megan Chen': { team: 'pandas', member: 'megan-chen' },
  'Vincenzo': { team: 'potato', member: 'vincenzo-sorcigli' },
  'Cody Pellerito': { team: 'potato', member: 'cody-pellerito' },
  'YetroC': { team: 'cactus', member: 'yetro-cheng' },
  'Krisha Patel': { team: 'orca', member: 'krisha-patel' },
  'Thomson Cheung': { team: 'cactus', member: 'thomson-cheung' },
  'Christopher Rella': { team: 'potato', member: 'christopher-rella' },
  'Amina Rhamatzada': { team: 'iris', member: 'amina-rhamatzada' },
  'sinanrah': { team: 'hawks', member: 'sinan-rahman' },
  '=': { team: 'pandas', member: 'joy-zou' },
  'MintGreenTZ': { team: 'iris', member: 'zeynep-tasoglu' },
  'chokra': { team: 'hawks', member: 'matthew-audain' },
};

// Commit type classification patterns
// Order matters - critical is checked first to identify core design work
const COMMIT_PATTERNS = {
  'critical': [
    // DHT stability and core P2P
    /\bdht\b.*\b(stab|reliab|persist|bootstrap|kademlia|routing)/i,
    /\bkademlia\b/i,
    /\bdht\s*(publish|lookup|discovery|provider)/i,
    /\bpeer\s*discovery\b/i,
    /\bbootstrap\s*node/i,
    // Bitswap and file exchange core
    /\bbitswap\b/i,
    /\bfile\s*(exchange|transfer|sharing)\s*(protocol|system|core)/i,
    /\bchunk(ed|ing)?\s*(transfer|exchange|verif)/i,
    /\bmulti.?source\s*download/i,
    // Payment core
    /\bpayment\s*(exchange|protocol|channel|system)/i,
    /\btransaction\s*(sign|verif|valid|process)/i,
    // Core protocol implementations (excluding relay)
    /\blibp2p\b/i,
    /\bprotocol\s*(implement|handler|negotiat)/i,
    // Core architecture
    /\bcore\s*(implement|architect|design|system)/i,
    /\bfoundation(al)?\b/i,
  ],
  'feature': [
    /^feat(\(.+\))?:/i,
    /^feature(\(.+\))?:/i,
    /^add(\(.+\))?:/i,
    /\bimplement\b/i,
    /\badd(ed|ing)?\b.*\b(feature|functionality|support)\b/i,
    /^feat\b/i,
  ],
  'bugfix': [
    /^fix(\(.+\))?:/i,
    /^bug(\(.+\))?:/i,
    /^hotfix(\(.+\))?:/i,
    /\bfix(ed|es|ing)?\b/i,
    /\bresolved?\b/i,
    /\bbug\b/i,
  ],
  'ui': [
    /\bui\b/i,
    /\bux\b/i,
    /\bstyle\b/i,
    /\bcss\b/i,
    /\blayout\b/i,
    /\bdesign\b/i,
    /\bresponsive\b/i,
    /\.svelte\b/i,
    /\bcomponent\b/i,
    /\bpage\b/i,
    /\bvisual\b/i,
  ],
  'refactor': [
    /^refactor(\(.+\))?:/i,
    /\brefactor(ed|ing)?\b/i,
    /\bclean(ed|ing)?\s*up\b/i,
    /\brestructur(e|ed|ing)\b/i,
    /\breorganiz(e|ed|ing)\b/i,
  ],
  'docs': [
    /^docs?(\(.+\))?:/i,
    /\bdocument(ation|ed|ing)?\b/i,
    /\breadme\b/i,
    /\bcomment(s|ed|ing)?\b/i,
    /\.md\b/i,
  ],
  'test': [
    /^test(\(.+\))?:/i,
    /\btest(s|ed|ing)?\b/i,
    /\bspec\b/i,
    /\bvitest\b/i,
    /\bjest\b/i,
  ],
  'chore': [
    /^chore(\(.+\))?:/i,
    /^build(\(.+\))?:/i,
    /^ci(\(.+\))?:/i,
    /\bdependenc(y|ies)\b/i,
    /\bconfig(uration)?\b/i,
    /\bsetup\b/i,
    /\bmerge\b/i,
  ],
  'performance': [
    /^perf(\(.+\))?:/i,
    /\bperformance\b/i,
    /\boptimiz(e|ation|ed|ing)\b/i,
    /\bspeed\b/i,
    /\bfaster\b/i,
  ],
  'security': [
    /^security(\(.+\))?:/i,
    /\bsecurity\b/i,
    /\bauth(entication|orization)?\b/i,
    /\bencrypt(ion|ed)?\b/i,
    /\bwallet\b/i,
  ],
  'network': [
    /\bdht\b/i,
    /\bp2p\b/i,
    /\bpeer\b/i,
    /\bnetwork\b/i,
    /\brelay\b/i,
    /\bbitswap\b/i,
    /\bbittorrent\b/i,
    /\bwebrtc\b/i,
    /\bftp\b/i,
  ],
  'blockchain': [
    /\bblockchain\b/i,
    /\bgeth\b/i,
    /\bmining\b/i,
    /\btransaction\b/i,
    /\bbalance\b/i,
  ],
};

/**
 * Parse author name/email to get team and member info
 */
function parseAuthor(authorName, authorEmail) {
  // First check direct mappings
  if (AUTHOR_MAPPINGS[authorName]) {
    return AUTHOR_MAPPINGS[authorName];
  }

  // Try to extract from email if it contains team prefix
  const emailMatch = authorEmail.match(/(\w+)-([a-z]+-[a-z]+)/i);
  if (emailMatch) {
    const possibleTeam = emailMatch[1].toLowerCase();
    if (TEAM_PREFIXES.includes(possibleTeam)) {
      return { team: possibleTeam, member: emailMatch[2].toLowerCase() };
    }
  }

  // Try to extract from author name if it contains team prefix
  for (const prefix of TEAM_PREFIXES) {
    const regex = new RegExp(`^${prefix}-(.+)$`, 'i');
    const match = authorName.match(regex);
    if (match) {
      return { team: prefix, member: match[1].toLowerCase().replace(/\s+/g, '-') };
    }
  }

  // Check GitHub noreply emails
  const noreplyMatch = authorEmail.match(/(\d+)\+([a-z]+-[a-z]+-[a-z]+)@users\.noreply\.github\.com/i);
  if (noreplyMatch) {
    const username = noreplyMatch[2];
    for (const prefix of TEAM_PREFIXES) {
      if (username.toLowerCase().startsWith(prefix + '-')) {
        const memberPart = username.substring(prefix.length + 1);
        return { team: prefix, member: memberPart.toLowerCase() };
      }
    }
  }

  // Return as unknown
  return { team: 'unknown', member: authorName.toLowerCase().replace(/\s+/g, '-').replace(/[^a-z0-9-]/g, '') };
}

/**
 * Classify commit type based on commit message
 */
function classifyCommit(message) {
  const types = [];

  // Check merge commits first
  if (/^Merge\s+(pull\s+request|branch|remote)/i.test(message)) {
    return ['merge'];
  }

  // Check each pattern category
  for (const [type, patterns] of Object.entries(COMMIT_PATTERNS)) {
    for (const pattern of patterns) {
      if (pattern.test(message)) {
        if (!types.includes(type)) {
          types.push(type);
        }
        break;
      }
    }
  }

  // Default to 'other' if no matches
  if (types.length === 0) {
    types.push('other');
  }

  return types;
}

/**
 * Get git log data
 */
function getGitLog() {
  console.log('Fetching git log...');
  const log = execSync(
    'git log --pretty=format:"%H|%an|%ae|%ad|%s" --date=short --all',
    { encoding: 'utf-8', maxBuffer: 50 * 1024 * 1024 }
  );

  return log.split('\n').filter(line => line.trim()).map(line => {
    const parts = line.split('|');
    if (parts.length >= 5) {
      return {
        hash: parts[0],
        author: parts[1],
        email: parts[2],
        date: parts[3],
        message: parts.slice(4).join('|'), // In case message contains |
      };
    }
    return null;
  }).filter(Boolean);
}

/**
 * Get file changes for each commit (lines added/removed)
 */
function getCommitStats() {
  console.log('Fetching commit stats...');
  const stats = execSync(
    'git log --pretty=format:"%H" --numstat --all',
    { encoding: 'utf-8', maxBuffer: 50 * 1024 * 1024 }
  );

  const commitStats = {};
  let currentHash = null;

  for (const line of stats.split('\n')) {
    if (/^[a-f0-9]{40}$/.test(line)) {
      currentHash = line;
      commitStats[currentHash] = { additions: 0, deletions: 0, files: 0 };
    } else if (currentHash && line.trim()) {
      const match = line.match(/^(\d+|-)\t(\d+|-)\t(.+)$/);
      if (match) {
        const additions = match[1] === '-' ? 0 : parseInt(match[1], 10);
        const deletions = match[2] === '-' ? 0 : parseInt(match[2], 10);
        commitStats[currentHash].additions += additions;
        commitStats[currentHash].deletions += deletions;
        commitStats[currentHash].files += 1;
      }
    }
  }

  return commitStats;
}

/**
 * Main analysis function
 */
function analyze() {
  const commits = getGitLog();
  const commitStats = getCommitStats();

  console.log(`Processing ${commits.length} commits...`);

  // Data structures for aggregation
  const teams = {};
  const members = {};
  const timeline = {};
  const typeStats = {};

  for (const commit of commits) {
    const { team, member } = parseAuthor(commit.author, commit.email);
    const types = classifyCommit(commit.message);
    const stats = commitStats[commit.hash] || { additions: 0, deletions: 0, files: 0 };
    const dateKey = commit.date; // YYYY-MM-DD
    const weekKey = getWeekKey(commit.date);

    // Initialize team
    if (!teams[team]) {
      teams[team] = {
        name: team,
        commits: 0,
        additions: 0,
        deletions: 0,
        files: 0,
        members: new Set(),
        types: {},
        timeline: {},
        firstCommit: commit.date,
        lastCommit: commit.date,
      };
    }

    // Initialize member
    const memberId = `${team}-${member}`;
    if (!members[memberId]) {
      members[memberId] = {
        id: memberId,
        team,
        name: member,
        displayName: formatDisplayName(member),
        commits: 0,
        additions: 0,
        deletions: 0,
        files: 0,
        types: {},
        timeline: {},
        firstCommit: commit.date,
        lastCommit: commit.date,
        messages: [],
      };
    }

    // Update team stats
    teams[team].commits++;
    teams[team].additions += stats.additions;
    teams[team].deletions += stats.deletions;
    teams[team].files += stats.files;
    teams[team].members.add(member);
    if (commit.date < teams[team].firstCommit) teams[team].firstCommit = commit.date;
    if (commit.date > teams[team].lastCommit) teams[team].lastCommit = commit.date;

    // Update member stats
    members[memberId].commits++;
    members[memberId].additions += stats.additions;
    members[memberId].deletions += stats.deletions;
    members[memberId].files += stats.files;
    if (commit.date < members[memberId].firstCommit) members[memberId].firstCommit = commit.date;
    if (commit.date > members[memberId].lastCommit) members[memberId].lastCommit = commit.date;

    // Store a sample of commit messages for this member
    if (members[memberId].messages.length < 50) {
      members[memberId].messages.push({
        date: commit.date,
        message: commit.message.substring(0, 100),
        types,
      });
    }

    // Update type stats
    for (const type of types) {
      teams[team].types[type] = (teams[team].types[type] || 0) + 1;
      members[memberId].types[type] = (members[memberId].types[type] || 0) + 1;
      typeStats[type] = (typeStats[type] || 0) + 1;
    }

    // Update timeline
    if (!teams[team].timeline[weekKey]) teams[team].timeline[weekKey] = 0;
    teams[team].timeline[weekKey]++;

    if (!members[memberId].timeline[weekKey]) members[memberId].timeline[weekKey] = 0;
    members[memberId].timeline[weekKey]++;

    if (!timeline[weekKey]) timeline[weekKey] = 0;
    timeline[weekKey]++;
  }

  // Convert Sets to arrays and calculate scores
  const teamList = Object.values(teams).map(t => ({
    ...t,
    members: Array.from(t.members),
    memberCount: t.members.size,
    score: calculateScore(t),
    avgCommitsPerMember: t.members.size > 0 ? Math.round(t.commits / t.members.size) : 0,
    summary: TEAM_SUMMARIES[t.name] || '',
  })).sort((a, b) => b.score - a.score);

  const memberList = Object.values(members).map(m => ({
    ...m,
    score: calculateScore(m),
    primaryType: getPrimaryType(m.types),
  })).sort((a, b) => b.score - a.score);

  // Calculate rankings
  teamList.forEach((t, i) => t.rank = i + 1);
  memberList.forEach((m, i) => m.rank = i + 1);

  // Add team rankings within each team
  const teamMemberRanks = {};
  for (const team of teamList) {
    const teamMembers = memberList.filter(m => m.team === team.name);
    teamMembers.sort((a, b) => b.score - a.score);
    teamMembers.forEach((m, i) => {
      teamMemberRanks[m.id] = i + 1;
    });
  }
  memberList.forEach(m => m.teamRank = teamMemberRanks[m.id] || 0);

  // Get most recent commit info
  const recentCommit = commits[0] ? {
    hash: commits[0].hash.substring(0, 8),
    fullHash: commits[0].hash,
    author: commits[0].author,
    date: commits[0].date,
    message: commits[0].message,
    types: classifyCommit(commits[0].message),
  } : null;

  // Generate summary
  const summary = {
    totalCommits: commits.length,
    totalTeams: teamList.filter(t => !['staff', 'external', 'unknown'].includes(t.name)).length,
    totalMembers: memberList.filter(m => !['staff', 'external', 'unknown'].includes(m.team)).length,
    dateRange: {
      start: commits[commits.length - 1]?.date || '',
      end: commits[0]?.date || '',
    },
    typeBreakdown: typeStats,
    timeline: Object.entries(timeline).sort((a, b) => a[0].localeCompare(b[0])),
    recentCommit,
  };

  return {
    summary,
    teams: teamList,
    members: memberList,
    generatedAt: new Date().toISOString(),
  };
}

/**
 * Calculate contribution score
 */
function calculateScore(entity) {
  const commitWeight = 1;
  const additionWeight = 0.01;
  const deletionWeight = 0.005;

  // Bonuses for high-value work
  const criticalBonus = 5.0;      // Critical core design work (DHT, bitswap, payment)
  const featureBonus = 2.0;       // New features are high value
  const bugfixBonus = 1.5;        // Bug fixes are important
  const networkBonus = 1.5;       // Core P2P/DHT work is complex
  const securityBonus = 1.5;      // Security work is critical
  const blockchainBonus = 1.2;    // Blockchain integration
  const testBonus = 1.0;          // Testing is valuable
  const docsBonus = 3.0;          // Documentation is highly valuable for the team
  const refactorBonus = 0.8;      // Refactoring improves code quality
  const performanceBonus = 1.2;   // Performance optimization

  // Penalties for lower-effort work
  const uiPenalty = -0.3;         // UI work is less complex
  const chorePenalty = -0.3;      // Chores are routine
  const otherPenalty = -0.2;      // Uncategorized commits
  const mergePenalty = -0.8;      // Merge commits are often automated

  let score = entity.commits * commitWeight;
  score += entity.additions * additionWeight;
  score += entity.deletions * deletionWeight;

  // Apply bonuses for high-value work
  score += (entity.types['critical'] || 0) * criticalBonus;
  score += (entity.types['feature'] || 0) * featureBonus;
  score += (entity.types['bugfix'] || 0) * bugfixBonus;
  score += (entity.types['network'] || 0) * networkBonus;
  score += (entity.types['security'] || 0) * securityBonus;
  score += (entity.types['blockchain'] || 0) * blockchainBonus;
  score += (entity.types['test'] || 0) * testBonus;
  score += (entity.types['docs'] || 0) * docsBonus;
  score += (entity.types['refactor'] || 0) * refactorBonus;
  score += (entity.types['performance'] || 0) * performanceBonus;

  // Apply penalties for lower-effort work
  score += (entity.types['ui'] || 0) * uiPenalty;
  score += (entity.types['chore'] || 0) * chorePenalty;
  score += (entity.types['other'] || 0) * otherPenalty;
  score += (entity.types['merge'] || 0) * mergePenalty;

  return Math.round(score * 10) / 10;
}

/**
 * Get the primary contribution type for a member
 */
function getPrimaryType(types) {
  const typeList = Object.entries(types)
    .filter(([t]) => t !== 'merge' && t !== 'other')
    .sort((a, b) => b[1] - a[1]);
  return typeList.length > 0 ? typeList[0][0] : 'other';
}

/**
 * Get week key from date string (YYYY-WW)
 */
function getWeekKey(dateStr) {
  const date = new Date(dateStr);
  const year = date.getFullYear();
  const firstDayOfYear = new Date(year, 0, 1);
  const daysSinceFirstDay = Math.floor((date - firstDayOfYear) / (24 * 60 * 60 * 1000));
  const weekNumber = Math.ceil((daysSinceFirstDay + firstDayOfYear.getDay() + 1) / 7);
  return `${year}-W${weekNumber.toString().padStart(2, '0')}`;
}

/**
 * Format member name for display
 */
function formatDisplayName(member) {
  return member
    .split('-')
    .map(word => word.charAt(0).toUpperCase() + word.slice(1))
    .join(' ');
}

// Team summaries based on commit analysis
const TEAM_SUMMARIES = {
  'turtle': `Team Turtle was the most prolific contributor this semester, with Jake Mallen leading as the top individual contributor across all teams. Their primary focus was on core file transfer infrastructure and protocol implementation. Jake implemented critical features including FTP protocol support, WebRTC data channel improvements, BitTorrent download functionality, and multi-seeder retry handling. The team fixed numerous bugs related to file downloads, chunk handling, and network synchronization. David Lai contributed significantly to FTP integration, fixing download bars, file discovery, and upload functionality. Alexander Joukov added auto-start functionality for DHT and Geth nodes, prevented duplicate seeder entries, and improved build times. The team also worked on mining rewards, Ethereum contract integration, ED2K protocol support, and wallet synchronization. Their debugging efforts included extensive logging for WebRTC transfers, fixing race conditions, and resolving deadlocks in the download system.`,

  'potato': `Team Potato focused heavily on infrastructure reliability, testing, and system robustness. Margaret Jin made substantial contributions to the backend, implementing a Rust chunk scheduler, peer health manager, reassembly manager with checksum validation, and comprehensive logging improvements with Tauri file sink integration. Nicholas Gitman worked on core system fixes and validation. Khannah Shaltiel contributed to the reputation system and peer selection service improvements. The team implemented input validation, storage path validation with platform-specific checks, and transaction balance fixes. Giovanni Mascetti enhanced storage path validation with platform-specific default paths. Cody Pellerito and Christopher Rella contributed to relay health monitoring and configuration. The team prioritized code quality with extensive unit testing and documentation, making the system more reliable and maintainable.`,

  'pandas': `Team Pandas concentrated on multi-source download capabilities and protocol integration. Sara Zhu (Qianhe Zhu) led efforts on the multi-source download integration, implementing FTP protocol handler improvements and fixing thread safety issues. Sandy Wu developed a unified download/upload interface for the ProtocolManager and implemented FTP data fetching and verification for the multi-source download system. Yvette Han contributed to feature development and blockchain integration. The team worked on chunk handling using ChunkInfo structs, ED2K data fetching and verification, and protocol manager improvements. Joy Zou and Megan Chen contributed to UI improvements and system features. Ashley Wu worked on various feature implementations. The team's work enabled the application to download files from multiple sources simultaneously, improving download speeds and reliability.`,

  'iris': `Team Iris was instrumental in BitTorrent protocol implementation and core feature development. Zeynep Tasoglu was a standout contributor, implementing BitTorrent state persistence, torrent state management, download queue logic, and DHT publishing for seeding. She also worked on client identification, exclusive download with public fallback logic, and preventing duplicate torrents. Fahim Jawad implemented working event emitting for both magnet link and torrent-based downloads, fixed magnet link display issues, and enabled proper torrent file downloads from the BitTorrent DHT. Sharanya Kataru contributed to reputation visualization and the exclusive download fallback system. Amina Rhamatzada also contributed to team efforts. The team's BitTorrent integration was crucial for enabling decentralized file sharing through the standard BitTorrent protocol.`,

  'eagles': `Team Eagles excelled in network infrastructure, DHT implementation, and WebRTC file transfers. Gilho Kim was the technical lead, redesigning the network page for better usability, implementing ED2K chunk verification, and building comprehensive WebRTC file transfer functionality including checkpoint persistence for download resume, flow control with ACK protocol, and streaming chunk writes for large files. He also improved NAT traversal with STUN server configuration and fixed DHT-related issues. Sieun Park focused on wallet UI improvements, mining controls during blockchain sync, and automatic corruption detection. Joseph Seo worked on WebRTC protocol selection. Stanley Lee added global download toast notifications, scroll position memory, and internationalization updates. Wilson Lin contributed to reachability UI and multi-seeder WebRTC retry handling. The team produced extensive documentation for their implementations.`,

  'orca': `Team Orca focused on wallet functionality, reputation systems, and blockchain integration. Grace Wang made significant contributions to the reputation system, implementing seeder reputation tracking, transaction logic for WebRTC payments, and fixing the Bitswap payment handler. She also enabled autorelay and fixed mDNS discovery issues. Belle Zheng worked on wallet state cleanup, removing auto-lock functionality, and disabling blockchain/mining menu items when disconnected. Amy Zhang fixed wallet balance updates on block mining events and wallet export functionality. Krisha Patel implemented wallet UI for importing on app start, fixed transaction history loading, and ensured downloaded files automatically get seeded. Ninad Nawrikar contributed to keystore management and reputation documentation. Mehadi Chowdhury and Lee Eisenberg contributed to various features. The team's work ensured smooth wallet operations and fair peer reputation tracking.`,

  'totoro': `Team Totoro specialized in DHT stability, bootstrap node management, and network reliability. Ming Lin was the primary contributor, fixing critical bootstrap issues, preventing bootstrap nodes from advertising on relay, implementing LRU-style eviction for relays, and adding UDP transport support. He also fixed mDNS peer discovery, disabled UDP temporarily to stabilize downloads, and improved headless CLI functionality. Aaron Purnawan implemented graceful Geth shutdown and fixed overly aggressive corruption detection. Angela Lee fixed search history dropdown clipping and contributed to DHT-related work. Brian Lin moved protocol selection to settings and updated documentation. Ethan Diep added UI and functionality for canceled downloads in download history and fixed reactivity issues. The team's focus on network stability was essential for reliable peer-to-peer connections.`,

  'walrus': `Team Walrus contributed significantly to testing, internationalization, and code quality. Terry Lim was a testing champion, adding comprehensive test suites for peer events (covering validation and deduplication), settings backup, download history, payment service, WebRTC service, DHT service, signaling service, and encryption services - totaling hundreds of test cases. Toni Liang focused on UI/UX improvements, implementing mobile-responsive features, QR code functionality, wallet UI enhancements, and adding French, Bengali, and Arabic language support. Chanul Dandeniya implemented the RequestFileAccess handler with key request protocol, chunk integrity verification, and refactored logging throughout the codebase. Michael Tso contributed to various fixes and merges. Lee Eisenberg worked on wallet import functionality. Ashish Jalwan contributed to default etherbase account settings.`,

  'hawks': `Team Hawks worked on download reliability, HTTP protocol improvements, and testing infrastructure. Adarsh Bharti fixed DHT issues related to relay circuit advertisement, preventing double circuits and nested relay advertisements. He also created a download fault injection test harness with mock HTTP server for testing. Samridh Samridh implemented restartable HTTP downloads with proper UI controls, CLI support, and backend integration. Joshua Boone developed HTTP range client for download restart baseline functionality. The team focused on making downloads more resilient to failures and network issues. Matthew Audain, Sinan Rahman, and Eliot Shytaj contributed to various fixes and features. Samuel Buena worked on file sharing encryption using multiple public keys. The team's fault-tolerant download system improved user experience significantly.`,

  'cactus': `Team Cactus focused on NAT traversal, protocol integration, and documentation. Chiu Choi implemented UPnP NAT traversal in the Rust backend with frontend integration, auto-enabling relay server on public IP detection, and automatic relay discovery via DHT. He also added shell scripts for running relay servers and testing on different platforms. Yetro Cheng was prolific in documentation, updating guides for FTP, HTTP protocol, BitTorrent implementation, bootstrap health integration, and WebRTC. He also implemented the transfer event bus integration, DHT health monitoring with auto-recovery, and the connection retry framework. Thomson Cheung contributed to various features. Despite being a smaller team, Cactus made significant contributions to network robustness and developer documentation that benefited the entire project.`,

  'whales': `Team Whales, though small with only two members, made focused contributions to network protocols and code quality. Aynur Tariq was the primary contributor, working on network-related features including refactoring, documentation, and critical fixes. The team contributed to DHT functionality, protocol improvements, and bug fixes across the codebase. Daniel Briskman contributed to documentation and testing efforts. Their work helped improve the overall stability and maintainability of the networking layer, with particular attention to code refactoring and ensuring consistent behavior across the P2P communication stack.`
};

// Run analysis
console.log('Starting contribution analysis...');
const results = analyze();

// Save results
const outputPath = path.join(__dirname, '..', 'contribution-data.json');
fs.writeFileSync(outputPath, JSON.stringify(results, null, 2));
console.log(`\nResults saved to ${outputPath}`);

// Print summary
console.log('\n=== CONTRIBUTION SUMMARY ===');
console.log(`Total commits: ${results.summary.totalCommits}`);
console.log(`Total teams: ${results.summary.totalTeams}`);
console.log(`Total members: ${results.summary.totalMembers}`);
console.log(`Date range: ${results.summary.dateRange.start} to ${results.summary.dateRange.end}`);

console.log('\n=== TOP 10 TEAMS ===');
results.teams.slice(0, 10).forEach(t => {
  console.log(`${t.rank}. ${t.name}: ${t.commits} commits, ${t.memberCount} members, score: ${t.score}`);
});

console.log('\n=== TOP 15 CONTRIBUTORS ===');
results.members.slice(0, 15).forEach(m => {
  console.log(`${m.rank}. ${m.displayName} (${m.team}): ${m.commits} commits, score: ${m.score}, primary: ${m.primaryType}`);
});

console.log('\n=== CONTRIBUTION TYPES ===');
Object.entries(results.summary.typeBreakdown)
  .sort((a, b) => b[1] - a[1])
  .forEach(([type, count]) => {
    console.log(`  ${type}: ${count}`);
  });
