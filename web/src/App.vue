<script setup>
import { ref, onMounted, computed } from 'vue';
import { useDark, useToggle } from '@vueuse/core';
import init, { parse, formats, version as wasmVersion } from 'xfina-wasm';
import { Sun, Moon, Github, HelpCircle, ChevronDown, Loader2, ArrowUp, ArrowDown, GitCommit, CheckCircle2, AlertTriangle, XCircle, MinusCircle, Activity, Upload, Lock, X, ExternalLink } from 'lucide-vue-next';

// Shadcn components
import { Button } from '@/components/ui/button';
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from '@/components/ui/accordion';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog';
import { getStoredAnalyticsLevel, setStoredAnalyticsLevel, updateAnalyticsState, trackParserEvent, LEVEL_OFF, LEVEL_ANONYMOUS } from '@/lib/analytics.js';
import StatementHeader from '@/components/StatementHeader.vue';
import PublicData from '@/components/PublicData.vue';
import { formatDate, formatDateTime } from '@/lib/format.js';

const analyticsLevel = ref(getStoredAnalyticsLevel());

const setAnalyticsLevel = (level) => {
    analyticsLevel.value = level;
    setStoredAnalyticsLevel(level);
};

const isDark = useDark();
const toggleDark = () => {
    isDark.value = !isDark.value;
    if (isDark.value) {
        document.documentElement.classList.add('dark');
    } else {
        document.documentElement.classList.remove('dark');
    }
};

onMounted(() => {
    if (isDark.value) {
        document.documentElement.classList.add('dark');
    } else {
        document.documentElement.classList.remove('dark');
    }
    updateAnalyticsState(analyticsLevel.value);
});

const wasmLoaded = ref(false);
const error = ref(null);
const availableFormats = ref([]);
const parserVersion = ref(null);

/**
 * Everything dropped in, and where each file got to.
 *
 * A statement says which institution issued it and what kind of account it is,
 * so there is nothing to pick before importing. What a file cannot answer is
 * its own password, and that is the only question left to ask.
 *
 * status: pending -> reading -> parsed | locked | failed
 */
const files = ref([]);
const activeId = ref(null);
const dropping = ref(false);
let nextId = 0;
// Whatever arrives next takes the view. Set when files are dropped or a locked
// one is opened, and cleared by the first of them that parses -- so importing
// a folder settles on its first readable statement instead of flicking through
// every one as they are read.
let focusNext = false;

const locked = computed(() => files.value.filter(f => f.status === 'locked'));
const failed = computed(() => files.value.filter(f => f.status === 'failed'));
const ready = computed(() => files.value.filter(f => f.status === 'parsed'));
const isProcessing = computed(() => files.value.some(f => f.status === 'reading' || f.status === 'pending'));
const readCount = computed(() => files.value.filter(f => f.status !== 'pending' && f.status !== 'reading').length);

/**
 * Two areas, never mixed: somebody's own statements, and data a publisher
 * hands to anyone. One drop zone feeds both -- each file says which it is --
 * and each has its own tab, so a pile of index history never buries a bank
 * statement, and a statement's password prompt never sits among NAVs.
 */
const view = ref('personal');
const areaOf = (entry) => entry.response?.area ?? 'personal';
const personalReady = computed(() => ready.value.filter(f => areaOf(f) === 'personal'));
const publicReady = computed(() => ready.value.filter(f => areaOf(f) === 'public'));

const active = computed(() => personalReady.value.find(f => f.id === activeId.value) ?? personalReady.value[0] ?? null);

// Every view reads the selected file's parse. Switching tabs is a re-render,
// not a re-parse: each file is read once, when it arrives.
const result = computed(() => active.value?.response ?? null);
const validationReport = computed(() => result.value?.validation ?? null);
const fileInfo = computed(() => result.value?.detection?.file ?? null);
const parseTime = computed(() => active.value?.parseTime ?? null);

const totalTxns = computed(() => result.value?.data?.transactions?.transaction?.length || 0);

const statementOf = (category) =>
    computed(() => (result.value?.category === category ? result.value.data : null));
const bankStatement = statementOf('bank_account');
const ccStatement = statementOf('credit_card');
const mfStatement = statementOf('mutual_funds');
const equityStatement = statementOf('intl_stocks');


const versionsData = ref(null);
const appVersion = import.meta.env.VITE_APP_VERSION || 'Unreleased';
// Every entry in versions.json is keyed by `minor`, and the unreleased build
// uses this literal -- so one string names this site in the registry and
// selects it in the dropdown. Released series are always X.Y, so nothing else
// can claim it.
const UNRELEASED = 'unreleased';
const isUnreleased = (series) => series.minor === UNRELEASED;
const activeMinor = appVersion !== 'Unreleased' ? appVersion.split('.').slice(0, 2).join('.') : UNRELEASED;
const isLocalhost = ref(false);

const allSeries = computed(() => versionsData.value?.series ?? []);
// The list is ordered -- releases newest first, unreleased last -- but match on
// the key rather than the index, so a half-written registry cannot end up
// labelling the unreleased build as the latest release.
const latestSeries = computed(() => allSeries.value.find(s => !isUnreleased(s)) ?? null);
const unreleasedSeries = computed(() => allSeries.value.find(isUnreleased) ?? null);
const pastSeries = computed(() =>
    allSeries.value.filter(s => !isUnreleased(s) && s !== latestSeries.value)
);
const currentSeries = computed(() => allSeries.value.find(s => s.minor === activeMinor) ?? null);

// versions.json is what the published sites go by: it names the commit each
// directory was built from, and correcting it there fixes the badge without a
// rebuild. The hash vite bakes in at build time is the fallback for local dev,
// where there is no versions.json to fetch.
const buildCommitHash = __COMMIT_HASH__;
const cleanCommitHash = computed(() =>
    (currentSeries.value?.commit || buildCommitHash || '').replace('*', '')
);
const shortCommitHash = computed(() => cleanCommitHash.value.substring(0, 7));

const selectedDropdownValue = computed(() => activeMinor);

const onVersionChange = (val) => {
    if (val === activeMinor) return;
    const target = allSeries.value.find(s => s.minor === val);
    if (!target) return;
    // The newest release is mirrored at the root, and that is the URL to hand
    // out for it -- not the versioned path it is also served from.
    window.location.href = target === latestSeries.value ? '/' : target.path;
};

onMounted(async () => {
    try {
        await init();
        wasmLoaded.value = true;
        // The accept string comes from the library, so adding a parser does not
        // mean editing this file.
        availableFormats.value = formats();
        // The version of the parsers actually running, which is not necessarily
        // the version of the page that loaded them.
        parserVersion.value = wasmVersion();
    } catch (e) {
        error.value = "Failed to load WebAssembly module: " + e;
    }

    isLocalhost.value = ['localhost', '127.0.0.1'].includes(window.location.hostname);

    // The published-version dropdown. Absolute, not relative: only the site
    // root carries versions.json, and a versioned build served from /0.4/
    // would otherwise ask for /0.4/versions.json, which does not exist.
    try {
        const res = await fetch("/versions.json");
        if (res.ok) {
            versionsData.value = await res.json();
        }
    } catch (e) {
        console.warn("Failed to fetch versions.json", e);
    }
});

// The extensions institutions actually use, straight from the library.
const getAcceptString = computed(() => {
    const extensions = new Set(
        availableFormats.value.filter(f => f.enabled).map(f => `.${f.extension}`)
    );
    return extensions.size ? [...extensions].join(',') : '*';
});

/** Takes a drop or a pick, ignoring files already in the list. */
const accept = (list) => {
    for (const file of list) {
        // Dropping the same folder twice is an ordinary slip, and one statement
        // in the list twice is confusing.
        if (files.value.some(f => f.name === file.name && f.size === file.size)) continue;
        files.value.push({
            id: nextId++,
            file,
            name: file.name,
            size: file.size,
            status: 'pending',
            password: '',
            response: null,
            parseTime: null,
            error: null,
        });
    }
    if (files.value.some(f => f.status === 'pending')) focusNext = true;
    drain();
};

const onPick = (event) => {
    accept(event.target.files ?? []);
    event.target.value = '';
};

const onDrop = (event) => {
    dropping.value = false;
    accept(event.dataTransfer?.files ?? []);
};

/**
 * One file at a time.
 *
 * A consolidated account statement takes seconds to read. Running six at once
 * would freeze the page and say nothing about which file is slow.
 */
const drain = async () => {
    if (files.value.some(f => f.status === 'reading')) return;
    const next = files.value.find(f => f.status === 'pending');
    if (!next) return;
    await read(next);
    await drain();
};

const read = async (entry) => {
    entry.status = 'reading';
    entry.error = null;
    // Yield so the row can render as "Reading..." before the parser blocks.
    await new Promise(resolve => setTimeout(resolve, 0));

    const start = performance.now();
    try {
        const bytes = new Uint8Array(await entry.file.arrayBuffer());
        const response = parse(bytes, {
            password: entry.password || null,
            filename: entry.name,
            modifiedTimestamp: entry.file.lastModified
                ? BigInt(Math.floor(entry.file.lastModified / 1000))
                : null,
        });
        const elapsed = performance.now() - start;
        entry.parseTime = (elapsed / 1000).toFixed(3);

        if (response.error) {
            entry.error = describeError(response.error);
            // A locked file is not a failure: nothing has been read out of it
            // yet, and a password is all it is waiting for.
            entry.status = response.error.kind === 'password_required'
                || response.error.kind === 'incorrect_password' ? 'locked' : 'failed';
            trackParserEvent(response.error.filename_hint || 'unknown', false,
                Math.round(elapsed), null, appVersion);
            return;
        }

        entry.response = response;
        entry.status = 'parsed';
        // The first file of a drop decides which tab is showing.
        if (focusNext) view.value = areaOf(entry);
        if (areaOf(entry) === 'personal' && (focusNext || activeId.value === null)) {
            activeId.value = entry.id;
        }
        focusNext = false;
        trackParserEvent(response.format, response.validation?.overall === 'passed',
            Math.round(elapsed), validationMetrics(response.validation), appVersion);
    } catch (e) {
        const elapsed = performance.now() - start;
        entry.parseTime = (elapsed / 1000).toFixed(3);
        entry.error = String(e);
        entry.status = 'failed';
        trackParserEvent('unknown', false, Math.round(elapsed), null, appVersion);
    }
};

/** Re-read a locked file once its password is supplied. */
const unlock = (entry) => {
    entry.status = 'pending';
    // You typed the password to read this one, so show it.
    focusNext = true;
    drain();
};

const remove = (entry) => {
    files.value = files.value.filter(f => f !== entry);
    if (activeId.value === entry.id) activeId.value = personalReady.value[0]?.id ?? null;
};

const clearAll = () => {
    files.value = [];
    activeId.value = null;
    error.value = null;
};

const CATEGORY_LABELS = {
    bank_account: 'Bank Account',
    credit_card: 'Credit Card',
    mutual_funds: 'Mutual Fund',
    intl_stocks: 'Intl Stocks',
    reference_rates: 'Reference Rates',
    market_data: 'Market Data',
};

// A fixed order, not the order files happened to be dropped in: the same pile
// should read the same way twice.
const CATEGORY_ORDER = ['bank_account', 'credit_card', 'mutual_funds', 'intl_stocks', 'reference_rates', 'market_data'];

/**
 * Imported statements, under the heading each belongs to.
 *
 * The heading carries the kind, so a card only has to say which account it is
 * and whose name is on it.
 */
const groupedReady = computed(() =>
    CATEGORY_ORDER
        .map(category => ({
            category,
            label: CATEGORY_LABELS[category],
            entries: personalReady.value
                .filter(e => e.response?.category === category)
                .sort(byPeriod),
        }))
        .filter(group => group.entries.length)
);

/** A period end as seconds, with a missing one sorting after every real date. */
const periodSeconds = (ts) =>
    ts === null || ts === undefined || ts === '' ? Infinity : Number(ts);

/**
 * Oldest period first, by start and then end, so a year of monthly statements
 * reads in order however the folder happened to list them. Ties keep the order
 * the files were dropped in.
 */
const byPeriod = (a, b) => {
    const pa = a.response?.data?.transactions;
    const pb = b.response?.data?.transactions;
    return (periodSeconds(pa?.startDate) - periodSeconds(pb?.startDate) || 0)
        || (periodSeconds(pa?.endDate) - periodSeconds(pb?.endDate) || 0);
};

/** Everything this build can read, under the same headings. */
const groupedFormats = computed(() =>
    CATEGORY_ORDER
        .map(category => ({
            category,
            label: CATEGORY_LABELS[category],
            entries: availableFormats.value.filter(f => f.category === category),
        }))
        .filter(group => group.entries.length)
);



/**
 * Institution and the account's last four digits.
 *
 * Some issuers print no account number at all -- ICICI cards are the case that
 * bites -- so the institution stands alone rather than trailing a dangling
 * separator.
 */
const accountOf = (entry) => {
    const institution = entry.response?.institution;
    if (!institution) return entry.name;
    const masked = entry.response?.data?.maskedAccNumber || '';
    const last4 = masked.replace(/[^0-9]/g, '').slice(-4);
    return last4 ? `${institution} - ${last4}` : institution;
};

const holderOf = (entry) =>
    entry.response?.data?.profile?.holders?.holder?.[0]?.name || '';

/** "<from> – <to>", or whichever end of it the statement has. */
const periodOf = (entry) => {
    const txns = entry.response?.data?.transactions;
    const from = txns?.startDate ? formatDate(txns.startDate) : '';
    const to = txns?.endDate ? formatDate(txns.endDate) : '';
    return from && to ? `${from} – ${to}` : from || to;
};

/**
 * What a locked file looks like, from its name alone.
 *
 * Nobody has read it -- that is what locked means -- so the filename is the
 * only evidence there is. Both halves come from the same matched pattern, so
 * they are present or absent together.
 */
const hintedAs = (err) => {
    const format = availableFormats.value.find(f => f.id === err.filename_hint);
    if (!format) return null;
    return { kind: CATEGORY_LABELS[format.category], institution: format.institution };
};

const article = (word) => (/^[AEIOU]/i.test(word) ? 'an' : 'a');

const describeError = (err) => {
    const hint = hintedAs(err);
    switch (err.kind) {
        case 'password_required':
            return hint
                ? `This looks like ${article(hint.kind)} ${hint.kind} statement from ${hint.institution}. Enter its password to continue.`
                : 'This file is password protected. Enter its password to continue.';
        case 'incorrect_password':
            return 'That password did not open the file.';
        case 'unrecognized_format':
            return `We read the file, but no parser recognised it as a supported statement or public data file${err.container ? ` (${err.container})` : ''}.`;
        default:
            return err.message;
    }
};

// A statement that prints no totals runs no summary checks, and must not look
// as though it passed them.
const summaryChecksRun = computed(() => {
    const summary = validationReport.value?.summary_level;
    return (summary?.declared?.checks?.length || 0) + (summary?.derived?.checks?.length || 0) > 0;
});

const validationMetrics = (report) => {
    if (!report || report.overall === 'passed') return null;
    const metrics = {};
    const txnsFailed = report.row_level?.failed_rows?.length || 0;
    if (txnsFailed > 0) metrics.txns_failed = txnsFailed;
    const declaredFailed = (report.summary_level?.declared?.checks || []).filter(c => !c.passed).length;
    if (declaredFailed > 0) metrics.declared_failed = declaredFailed;
    const derivedFailed = (report.summary_level?.derived?.checks || []).filter(c => !c.passed).length;
    if (derivedFailed > 0) metrics.derived_failed = derivedFailed;
    return Object.keys(metrics).length ? metrics : null;
};

const getCurrencySymbol = () => {
    // International brokerage is the only category we report in dollars, and
    // the parse tells us which category this is.
    return result.value?.category === 'intl_stocks' ? '$' : '₹';
};

const formatCurrency = (val) => {
    if (val === null || val === undefined) return '-';
    const num = Number(val);
    const formatted = Math.abs(num).toLocaleString('en-IN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
    return (num < 0 ? '\u2011' : '') + getCurrencySymbol() + formatted;
};


const formatUnits = (val) => {
    if (val === null || val === undefined) return '-';
    const num = Number(val);
    if (num === 0) return '0';
    return num.toLocaleString('en-IN', { minimumFractionDigits: 3, maximumFractionDigits: 3 });
};

const formatNumber = (val) => {
    if (val === null || val === undefined) return '-';
    return Number(val).toLocaleString('en-IN', { minimumFractionDigits: 0, maximumFractionDigits: 4 });
};

const hasRewards = (stmt) => {
    if (!stmt?.summary?.xfina?.rewardPointsSummary) return false;
    const s = stmt.summary.xfina.rewardPointsSummary;
    return s.openingBalance !== 0 || 
           s.earned !== 0 || 
           s.disbursed !== 0 || 
           s.adjustedLapsed !== 0 || 
           s.closingBalance !== 0 || 
           s.defaultRewards !== 0;
};

/** Reward points taken back by reversed transactions, as a positive count. */
const reversedPoints = (stmt) =>
    (stmt?.transactions?.transaction || [])
        .map(t => t.xfina?.rewardPoints)
        .filter(p => p < 0)
        .reduce((sum, p) => sum - p, 0);

const getAssetTransactions = (holding) => {
    if (!equityStatement.value?.transactions?.transaction) return [];
    const txns = equityStatement.value.transactions.transaction.filter(txn => 
        txn.symbol === holding.description || 
        txn.symbol === holding.issuerName || 
        txn.isin === holding.isin ||
        txn.companyName === holding.issuerName
    );
    
    let currentBalance = holding.xfina?.openingBalance || 0;
    
    return txns.map(txn => {
        if (txn.type === 'BUY') {
            currentBalance += Number(txn.units || 0);
        } else if (txn.type === 'SELL') {
            currentBalance -= Number(txn.units || 0);
        }
        return {
            ...txn,
            _runningBalance: currentBalance
        };
    });
};

const camsGroupedAssets = computed(() => {
    if (!mfStatement.value?.summary?.investment?.holdings?.holding) return [];
    
    const txnsByKey = {};
    if (mfStatement.value.transactions?.transaction) {
        for (const txn of mfStatement.value.transactions.transaction) {
            const key = `${txn.isin || 'noisin'}-${txn.xfina?.folioNo || 'nofolio'}`;
            if (!txnsByKey[key]) txnsByKey[key] = [];
            txnsByKey[key].push(txn);
        }
    }

    return mfStatement.value.summary.investment.holdings.holding.map(h => {
        const key = `${h.isin || 'noisin'}-${h.folioNo || 'nofolio'}`;
        const txns = txnsByKey[key] || [];
        
        return {
            isin: h.isin,
            name: h.xfina?.schemeName || 'Unknown Scheme',
            folioNo: h.folioNo,
            registrar: h.registrar,
            advisor: h.xfina?.advisor,
            kyc: h.xfina?.kyc,
            panKyc: h.xfina?.panKyc,
            nominees: h.xfina?.nominees,
            transactions: txns,
            
            periodBuyUnits: h.xfina?.periodBuyUnits || 0,
            periodBuyCount: h.xfina?.periodBuyCount || 0,
            periodSellUnits: h.xfina?.periodSellUnits || 0,
            periodSellCount: h.xfina?.periodSellCount || 0,
            closingBalance: h.units || 0,
            openingBalance: h.xfina?.openingBalance !== undefined ? h.xfina.openingBalance : 0,
            
            nav: h.nav || h.rate || 0,
            navDate: h.xfina?.navDate,
            marketValue: h.xfina?.currentValue || 0,
            totalInvested: h.xfina?.totalInvested || 0,
            unrealizedPl: h.xfina?.unrealizedPl || 0
        };
    });
});
</script>

<template>
  <div class="min-h-screen bg-background text-foreground p-8 font-sans transition-colors duration-200">
    <div class="max-w-6xl mx-auto space-y-8">
      
      <!-- Header -->
      <div class="flex flex-col md:flex-row md:justify-between md:items-start gap-4">
        <div class="flex items-start gap-5">
          <a href="." class="hover:opacity-80 transition-opacity flex-shrink-0 cursor-pointer">
            <img src="/favicon.svg" alt="Xfina Logo" class="w-16 h-16" />
          </a>
          <div class="space-y-2">
            <div class="flex flex-col sm:flex-row sm:items-center gap-3 sm:gap-4">
              <a href="." class="hover:text-primary transition-colors cursor-pointer">
                <h1 class="text-3xl font-bold tracking-tight">Xfina</h1>
              </a>
              <div class="flex items-center">
                <Select :key="versionsData ? 'loaded' : 'loading'" :modelValue="selectedDropdownValue" @update:modelValue="onVersionChange">
                  <SelectTrigger class="w-[140px] h-9 border-border bg-background shadow-sm rounded-r-none focus:z-10 focus:ring-1">
                    <SelectValue placeholder="Version" />
                  </SelectTrigger>
                  <SelectContent v-if="versionsData">
                    <SelectGroup>
                      <SelectItem 
                        v-if="latestSeries" 
                        :value="latestSeries.minor"
                      >
                        {{ latestSeries.minor }}.x (Latest)
                      </SelectItem>
                      <SelectItem 
                        v-for="series in pastSeries" 
                        :key="series.minor" 
                        :value="series.minor"
                      >
                        {{ series.minor }}.x
                      </SelectItem>
                      <SelectItem 
                        v-if="unreleasedSeries" 
                        :value="unreleasedSeries.minor"
                      >
                        Unreleased
                      </SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
                <a v-if="shortCommitHash" 
                   :href="`https://github.com/xfina-dev/xfina/commit/${cleanCommitHash}`" 
                   target="_blank" 
                   rel="noopener noreferrer" 
                   class="no-underline relative z-0">
                   <Button variant="outline" class="flex items-center gap-1.5 h-9 px-3 text-xs font-mono text-muted-foreground hover:text-foreground shadow-sm rounded-l-none border-l-0">
                     <GitCommit class="w-3.5 h-3.5" />
                     {{ shortCommitHash }}
                   </Button>
                </a>
              </div>
            </div>
            <p class="text-muted-foreground mt-2 leading-relaxed">
              e<strong>X</strong>tract <strong>fina</strong>ncial statements entirely in your browser with Rust/Wasm<br />
              Fast, private, zero-setup, and without uploading your files to any server.
            </p>
          </div>
        </div>
        <div class="flex items-center space-x-3">
          <a href="https://sakthipriyan.com/building-wealth" target="_blank" rel="noopener noreferrer" class="no-underline">
            <Button variant="outline" class="h-9 px-3 font-medium text-foreground">sakthipriyan.com</Button>
          </a>
          <a href="https://github.com/xfina-dev/xfina" target="_blank" rel="noopener noreferrer" class="no-underline" title="GitHub Repository">
            <Button variant="outline" size="icon">
              <Github class="h-[1.2rem] w-[1.2rem] text-foreground" />
              <span class="sr-only">GitHub Repository</span>
            </Button>
          </a>
          <Dialog>
            <DialogTrigger as-child>
              <Button variant="outline" size="icon" title="Privacy & Analytics">
                <Activity class="h-[1.2rem] w-[1.2rem] text-foreground" />
                <span class="sr-only">Privacy & Analytics</span>
              </Button>
            </DialogTrigger>
            <DialogContent class="sm:max-w-3xl">
              <DialogHeader>
                <DialogTitle>Privacy &amp; Analytics</DialogTitle>
                <DialogDescription>
                  <strong>Help Improve Xfina.</strong> Choose how you'd like to help us make Xfina better.
                </DialogDescription>
              </DialogHeader>
              <div class="space-y-4 py-4">
                <div class="flex items-center space-x-2 p-2 border rounded-md cursor-pointer hover:bg-muted" 
                     :class="{'border-primary bg-primary/5': analyticsLevel === LEVEL_ANONYMOUS}"
                     @click="setAnalyticsLevel(LEVEL_ANONYMOUS)">
                  <div class="flex-1 w-full">
                    <Accordion type="single" collapsible class="w-full">
                      <AccordionItem value="payload" class="border-b-0">
                        
                        <div class="flex items-center justify-between w-full">
                          <div class="font-semibold text-base flex items-center">
                            Anonymous Usage Statistics 
                            <span class="bg-primary/10 text-primary border border-primary/20 px-2 py-0.5 rounded-md text-[10px] uppercase font-bold ml-2 tracking-wide">Recommended</span>
                          </div>
                          
                          <div @click.stop>
                            <AccordionTrigger class="group hover:no-underline p-0 data-[state=open]:border-b-0">
                              <span class="sr-only">View exact payload details</span>
                              <template #icon>
                                <div class="flex items-center gap-1.5 text-xs font-mono bg-primary/10 text-primary pl-2.5 pr-2 py-1.5 rounded shrink-0 ml-2 hover:bg-primary/20 transition-colors">
                                  <span>Payload</span>
                                  <ChevronDown class="h-4 w-4 transition-transform duration-200 group-data-[state=open]:rotate-180" />
                                </div>
                              </template>
                            </AccordionTrigger>
                          </div>
                        </div>

                        <div class="text-sm text-muted-foreground mt-2">
                          <p>Help us improve parser quality and performance, and catch broken statement formats by sharing anonymous telemetry. <strong>No personal or financial data is collected.</strong> No file contents, transaction descriptions, financial values, or account numbers are ever included.</p>
                        </div>
                        
                        <div class="mt-3" @click.stop>
                          <AccordionContent>
                              <div class="max-h-[300px] overflow-y-auto pr-2">
                                <div class="rounded-md border overflow-hidden bg-background">
                                  <table class="w-full text-left text-sm">
                                    <thead class="bg-muted/50 text-muted-foreground">
                                      <tr>
                                        <th class="px-2 py-1.5 font-medium border-b whitespace-nowrap">Query Param</th>
                                        <th class="px-2 py-1.5 font-medium border-b w-full">Description</th>
                                        <th class="px-2 py-1.5 font-medium border-b w-1/4">Example Value</th>
                                      </tr>
                                    </thead>
                                    <tbody class="divide-y">
                                      <tr>
                                        <td class="px-2 py-1.5 font-mono text-primary/80">v</td>
                                        <td class="px-2 py-1.5">Standard GA4 protocol version</td>
                                        <td class="px-2 py-1.5 font-mono">2</td>
                                      </tr>
                                      <tr>
                                        <td class="px-2 py-1.5 font-mono text-primary/80">tid</td>
                                        <td class="px-2 py-1.5">Google Analytics measurement ID</td>
                                        <td class="px-2 py-1.5 font-mono">G-WZEYQGS8PE</td>
                                      </tr>
                                      <tr>
                                        <td class="px-2 py-1.5 font-mono text-primary/80">cid</td>
                                        <td class="px-2 py-1.5">Randomly generated session ID. Resets on every page load; never stored.</td>
                                        <td class="px-2 py-1.5 font-mono">433214986.1786291736</td>
                                      </tr>
                                      <tr>
                                        <td class="px-2 py-1.5 font-mono text-primary/80">en</td>
                                        <td class="px-2 py-1.5">Event Name</td>
                                        <td class="px-2 py-1.5 font-mono">parser_usage</td>
                                      </tr>
                                      <tr>
                                        <td class="px-2 py-1.5 font-mono text-primary/80">ep.app_version</td>
                                        <td class="px-2 py-1.5">Version of the web app and parsers</td>
                                        <td class="px-2 py-1.5 font-mono">{{ appVersion }}</td>
                                      </tr>
                                      <tr>
                                        <td class="px-2 py-1.5 font-mono text-primary/80">ep.parser_type</td>
                                        <td class="px-2 py-1.5">The type of statement being parsed</td>
                                        <td class="px-2 py-1.5 font-mono">icici_ba</td>
                                      </tr>
                                      <tr>
                                        <td class="px-2 py-1.5 font-mono text-primary/80">ep.success</td>
                                        <td class="px-2 py-1.5">True if no math/validation errors occurred</td>
                                        <td class="px-2 py-1.5 font-mono">false</td>
                                      </tr>
                                      <tr>
                                        <td class="px-2 py-1.5 font-mono text-primary/80">epn.parse_time_ms</td>
                                        <td class="px-2 py-1.5">Time taken to process the file locally</td>
                                        <td class="px-2 py-1.5 font-mono">2</td>
                                      </tr>
                                      <tr>
                                        <td class="px-2 py-1.5 font-mono text-primary/80">epn.txns_failed</td>
                                        <td class="px-2 py-1.5">Total failed transactions <i>(&gt; 0)</i></td>
                                        <td class="px-2 py-1.5 font-mono">3</td>
                                      </tr>
                                      <tr>
                                        <td class="px-2 py-1.5 font-mono text-primary/80">epn.declared_failed</td>
                                        <td class="px-2 py-1.5">Total failed declared checks <i>(&gt; 0)</i></td>
                                        <td class="px-2 py-1.5 font-mono">2</td>
                                      </tr>
                                      <tr>
                                        <td class="px-2 py-1.5 font-mono text-primary/80">epn.derived_failed</td>
                                        <td class="px-2 py-1.5">Total failed derived checks <i>(&gt; 0)</i></td>
                                        <td class="px-2 py-1.5 font-mono">1</td>
                                      </tr>
                                    </tbody>
                                  </table>
                                </div>
                                <div class="rounded-md border mt-4 overflow-hidden bg-background">
                                  <table class="w-full text-left text-sm">
                                    <thead class="bg-muted/50 text-muted-foreground">
                                      <tr>
                                        <th class="px-2 py-1.5 font-medium border-b">Actual Request URL (Example)</th>
                                      </tr>
                                    </thead>
                                    <tbody>
                                      <tr>
                                        <td class="p-0">
                                          <pre class="p-2 text-xs overflow-x-auto font-mono text-muted-foreground"><code>https://www.google-analytics.com/g/collect?
  v=2&amp;
  tid=G-WZEYQGS8PE&amp;
  cid=433214986.1786291736&amp;
  en=parser_usage&amp;
  ep.app_version=Unreleased&amp;
  ep.parser_type=icici_ba&amp;
  ep.success=false&amp;
  epn.parse_time_ms=2&amp;
  epn.txns_failed=3&amp;
  epn.declared_failed=2&amp;
  epn.derived_failed=1</code></pre>
                                        </td>
                                      </tr>
                                    </tbody>
                                  </table>
                                </div>
                              </div>
                            </AccordionContent>
                          </div>
                        </AccordionItem>
                      </Accordion>
                    </div>
                  </div>
                <div class="flex items-center space-x-2 p-2 border rounded-md cursor-pointer hover:bg-muted"
                     :class="{'border-primary bg-primary/5': analyticsLevel === LEVEL_OFF}"
                     @click="setAnalyticsLevel(LEVEL_OFF)">
                  <div class="flex-1">
                    <div class="font-semibold text-base">Zero Usage Statistics</div>
                    <div class="text-sm text-muted-foreground mt-1">Opt-out completely. We respect your privacy, and no telemetry requests will be sent from your browser. However, without anonymous telemetry, it may take us longer to discover broken statement formats, fix parsing bugs, and improve parser quality and performance.</div>
                  </div>
                </div>
              </div>
            </DialogContent>
          </Dialog>
          <Button variant="outline" size="icon" @click="toggleDark()" title="Toggle Theme">
            <Sun v-if="isDark" class="h-[1.2rem] w-[1.2rem] text-foreground" />
            <Moon v-else class="h-[1.2rem] w-[1.2rem] text-foreground" />
            <span class="sr-only">Toggle theme</span>
          </Button>
        </div>
      </div>
      
      <!-- Error Message -->
      <div v-if="error" class="p-4 bg-destructive/10 border border-destructive/20 rounded-md text-destructive">
        {{ error }}
      </div>
      
      <div v-if="!wasmLoaded" class="text-muted-foreground animate-pulse">Loading WebAssembly module...</div>

      <!-- Import: one drop zone, however many statements. Nothing to pick
           first -- each file says which institution issued it and what kind of
           account it is. -->
      <Card v-if="wasmLoaded" class="bg-card border-border shadow-sm">
        <CardHeader class="flex flex-row items-start justify-between space-y-0 pb-4">
          <div class="space-y-1.5">
            <CardTitle class="flex items-center gap-2">
              <span>Import files</span>
              <!-- The version of the parsers actually running, which is not
                   necessarily the version of the page that loaded them. -->
              <span
                v-if="parserVersion"
                class="rounded-md border border-border bg-muted px-1.5 py-0.5 font-mono text-[11px] font-normal text-muted-foreground"
                title="Parser version"
              >{{ parserVersion }}</span>
            </CardTitle>
            <CardDescription>
              Drop a whole folder in at once: statements, or price history from the publishers listed. Each file is read in your browser and nothing is uploaded.
              <Dialog>
                <DialogTrigger as-child>
                  <button class="underline underline-offset-4 hover:text-foreground">See supported formats</button>
                </DialogTrigger>
                <DialogContent class="sm:max-w-3xl">
                  <DialogHeader>
                    <DialogTitle>Supported files</DialogTitle>
                    <DialogDescription>
                      Where to download each one, and how to reach it. Read straight from this build, so the list cannot drift from what the parsers actually do.
                    </DialogDescription>
                  </DialogHeader>
                  <div class="max-h-[60vh] space-y-5 overflow-y-auto py-2">
                    <div v-for="group in groupedFormats" :key="group.category" class="space-y-2">
                      <p class="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                        {{ group.label }}
                      </p>
                      <div class="divide-y divide-border rounded-md border border-border">
                        <div
                          v-for="format in group.entries"
                          :key="format.id"
                          class="flex items-start justify-between gap-3 px-3 py-2.5"
                        >
                          <div class="min-w-0">
                            <!-- The link and the trail below it are the whole
                                 point of this dialog: reading a statement is
                                 easy once you have it, and finding where the
                                 institution hides the download is not. -->
                            <span class="flex items-baseline gap-2">
                              <a
                                :href="format.download_url"
                                target="_blank"
                                rel="noopener noreferrer"
                                class="inline-flex items-center gap-1 text-sm font-medium hover:underline"
                              >
                                <span class="truncate">{{ format.institution }}</span>
                                <ExternalLink class="h-3 w-3 shrink-0 text-muted-foreground" />
                              </a>
                              <!-- The same id the CLI takes for --as. -->
                              <span class="shrink-0 font-mono text-[11px] text-muted-foreground">{{ format.id }}</span>
                            </span>
                            <p class="mt-0.5 text-[11px] leading-snug text-muted-foreground">
                              {{ format.download_path }}
                            </p>
                          </div>
                          <div class="flex shrink-0 items-center gap-2">
                            <!-- One badge: the file type, with a lock when
                                 nothing can be read out of it until a password
                                 opens it. -->
                            <span
                              class="flex items-center gap-1 rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] uppercase text-muted-foreground"
                              :title="format.password_protected ? 'Password protected' : null"
                            >
                              <Lock v-if="format.password_protected" class="h-3 w-3 shrink-0" />
                              <span>{{ format.extension }}</span>
                            </span>
                            <span
                              v-if="!format.enabled"
                              class="rounded bg-muted px-1.5 py-0.5 text-[11px] text-muted-foreground"
                            >not built</span>
                            <CheckCircle2 v-else class="h-4 w-4 text-emerald-500" />
                          </div>
                        </div>
                      </div>
                    </div>
                  </div>
                </DialogContent>
              </Dialog>
            </CardDescription>
          </div>
          <div v-if="isProcessing" class="flex items-center text-sm font-medium text-muted-foreground gap-2 whitespace-nowrap mt-0.5">
            <span>Reading {{ readCount }} of {{ files.length }}…</span>
            <Loader2 class="h-4 w-4 animate-spin" />
          </div>
          <Button v-else-if="files.length" variant="ghost" size="sm" class="text-muted-foreground" @click="clearAll">Clear all</Button>
        </CardHeader>
        <CardContent>
          <label
            class="flex cursor-pointer flex-col items-center justify-center gap-2 rounded-xl border-2 border-dashed px-6 py-12 text-center transition-colors"
            :class="dropping ? 'border-primary bg-primary/5' : 'border-border hover:border-primary/40 hover:bg-muted/30'"
            @dragover.prevent="dropping = true"
            @dragleave.prevent="dropping = false"
            @drop.prevent="onDrop"
          >
            <Upload class="h-7 w-7 text-muted-foreground" />
            <span class="text-sm font-semibold">Drop files here, or click to browse</span>
            <span class="text-xs text-muted-foreground">
              Bank, credit card, mutual fund and brokerage statements, and public price, NAV and index history &mdash; Excel, CSV or PDF.
            </span>
            <input type="file" multiple class="hidden" :accept="getAcceptString" @change="onPick" />
          </label>
        </CardContent>
      </Card>

      <!-- Everything still waiting on the person who dropped the files. Locked
           files lead: nothing has been read out of them, so the filename is all
           there is to go on until a password opens one. -->
      <Card v-if="locked.length || failed.length" class="bg-card border-border shadow-sm">
        <CardHeader class="pb-4">
          <CardTitle class="text-base">
            Action required
            <span class="ml-1 font-normal text-muted-foreground">({{ locked.length + failed.length }})</span>
          </CardTitle>
          <CardDescription v-if="locked.length">
            Some statements are password protected. Nothing can be read out of them until they are opened.
          </CardDescription>
        </CardHeader>
        <CardContent class="space-y-3">
          <div
            v-for="entry in locked"
            :key="entry.id"
            class="flex flex-col gap-3 rounded-md border border-border p-3 sm:flex-row sm:items-center"
          >
            <Lock class="h-4 w-4 shrink-0 text-muted-foreground" />
            <div class="min-w-0 flex-1">
              <p class="truncate font-mono text-xs" :title="entry.name">{{ entry.name }}</p>
              <p class="mt-0.5 text-xs text-muted-foreground">{{ entry.error }}</p>
            </div>
            <form class="flex w-full gap-2 sm:w-auto" autocomplete="off" @submit.prevent="unlock(entry)">
              <Input
                v-model="entry.password"
                type="password"
                placeholder="Password"
                class="h-9 w-full sm:w-48 bg-background"
              />
              <Button type="submit" size="sm" :disabled="!entry.password">Unlock</Button>
              <!-- A password you do not have is a reason to drop the file, not
                   to be stuck on it. -->
              <Button type="button" variant="ghost" size="sm" class="px-2" @click="remove(entry)">
                <X class="h-4 w-4" />
              </Button>
            </form>
          </div>

          <div
            v-for="entry in failed"
            :key="entry.id"
            class="flex items-center gap-3 rounded-md border border-destructive/30 bg-destructive/5 p-3"
          >
            <XCircle class="h-4 w-4 shrink-0 text-destructive" />
            <div class="min-w-0 flex-1">
              <p class="truncate font-mono text-xs" :title="entry.name">{{ entry.name }}</p>
              <p class="mt-0.5 text-xs text-muted-foreground">{{ entry.error }}</p>
            </div>
            <Button variant="ghost" size="sm" class="px-2" @click="remove(entry)">
              <X class="h-4 w-4" />
            </Button>
          </div>
        </CardContent>
      </Card>

      <!-- The two areas. Counts say where a drop went without opening it. -->
      <div v-if="wasmLoaded" class="flex gap-1 border-b border-border" role="tablist">
        <button
          v-for="tab in [
            { id: 'personal', label: 'Personal statements', count: personalReady.length },
            { id: 'public', label: 'Public data', count: publicReady.length },
          ]"
          :key="tab.id"
          role="tab"
          :aria-selected="view === tab.id"
          class="-mb-px border-b-2 px-4 py-2 text-sm font-medium transition-colors"
          :class="view === tab.id
            ? 'border-primary text-foreground'
            : 'border-transparent text-muted-foreground hover:text-foreground'"
          @click="view = tab.id"
        >
          {{ tab.label }}
          <span v-if="tab.count" class="ml-1.5 rounded bg-muted px-1.5 py-0.5 text-[11px] tabular-nums text-muted-foreground">{{ tab.count }}</span>
        </button>
      </div>

      <PublicData v-if="view === 'public'" :entries="publicReady" :formats="availableFormats" />

      <template v-else>
      <!-- One card per statement read, grouped by the kind of account. A row
           that scrolls sideways hides whatever did not fit, and the whole
           point of importing a folder at once is seeing the pile. -->
      <div v-if="personalReady.length > 1" class="space-y-5">
        <div v-for="group in groupedReady" :key="group.category" class="space-y-2">
          <p class="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            {{ group.label }} <span class="font-normal">({{ group.entries.length }})</span>
          </p>
          <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            <button
              v-for="entry in group.entries"
              :key="entry.id"
              class="flex flex-col gap-0.5 rounded-lg border p-3 text-left transition-colors"
              :class="active?.id === entry.id
                ? 'border-primary bg-primary/5 ring-1 ring-primary/20'
                : 'border-border hover:border-primary/40 hover:bg-muted/40'"
              @click="activeId = entry.id"
            >
              <span class="truncate text-sm font-semibold leading-tight" :title="accountOf(entry)">
                {{ accountOf(entry) }}
              </span>
              <span
                v-if="holderOf(entry)"
                class="truncate text-xs leading-tight text-muted-foreground"
                :title="holderOf(entry)"
              >{{ holderOf(entry) }}</span>
              <span
                v-if="periodOf(entry)"
                class="truncate text-xs leading-tight tabular-nums text-muted-foreground"
                :title="periodOf(entry)"
              >{{ periodOf(entry) }}</span>
            </button>
          </div>
        </div>

        <!-- Everything below belongs to the one statement selected above. -->
        <hr class="border-border" />
      </div>


      <!-- Status Bar -->
      <div v-if="fileInfo" class="grid grid-cols-1 lg:grid-cols-3 gap-4 mb-2">
        <Card class="bg-card text-card-foreground shadow-sm border flex flex-col justify-center p-3 px-4">
          <span class="text-[10px] text-muted-foreground uppercase tracking-wider font-semibold mb-0.5">File Name</span>
          <span class="font-medium text-sm truncate" :title="fileInfo.name">{{ fileInfo.name }}</span>
        </Card>
        
        <Card class="bg-card text-card-foreground shadow-sm border flex items-center justify-between p-3 px-4">
          <div class="flex flex-col">
            <span class="text-[10px] text-muted-foreground uppercase tracking-wider font-semibold mb-0.5">Size</span>
            <span class="font-mono text-sm">{{ (fileInfo.size / 1024).toFixed(1) }} KB</span>
          </div>
          <div class="flex flex-col text-right">
            <span class="text-[10px] text-muted-foreground uppercase tracking-wider font-semibold mb-0.5">Last Modified</span>
            <span class="font-medium text-sm">{{ formatDateTime(fileInfo.modified_timestamp) }}</span>
          </div>
        </Card>
        
        <Card class="bg-card text-card-foreground shadow-sm border flex items-center justify-between p-3 px-4">
          <div class="flex flex-col">
            <span class="text-[10px] text-muted-foreground uppercase tracking-wider font-semibold mb-0.5">Validation Status</span>
            
            <div v-if="validationReport?.overall" class="flex items-center gap-3">
              <!-- Summary Badge -->
              <TooltipProvider>
                <Tooltip>
                  <TooltipTrigger class="cursor-help flex items-center gap-1.5 font-semibold text-sm w-fit"
                       :class="{
                         'text-muted-foreground': !summaryChecksRun,
                         'text-emerald-500': summaryChecksRun && validationReport.summary_level?.passed,
                         'text-amber-500': summaryChecksRun && !validationReport.summary_level?.passed && validationReport.summary_level?.declared?.passed,
                         'text-destructive': summaryChecksRun && !validationReport.summary_level?.declared?.passed
                       }">
                    <!-- Nothing to reconcile against is not a pass. -->
                    <MinusCircle v-if="!summaryChecksRun" class="w-4 h-4 opacity-70" />
                    <CheckCircle2 v-else-if="validationReport.summary_level?.passed" class="w-4 h-4" />
                    <!-- Only derived checks failed: a warning, not a failure. -->
                    <AlertTriangle v-else-if="validationReport.summary_level?.declared?.passed" class="w-4 h-4" />
                    <XCircle v-else class="w-4 h-4" />
                    <span>Summary</span>
                  </TooltipTrigger>
                  <TooltipContent side="bottom" class="p-3 max-w-sm">
                    <div class="space-y-3 text-sm">
                      <div v-if="validationReport.summary_level?.declared?.checks?.length > 0">
                        <div class="font-semibold text-foreground mb-0.5">Declared Validations</div>
                        <div class="text-muted-foreground">{{ validationReport.summary_level.declared.checks.filter(c => c.passed).length }} / {{ validationReport.summary_level.declared.checks.length }} checks passed</div>
                      </div>
                      <div v-if="validationReport.summary_level?.derived?.checks?.length > 0">
                        <div class="font-semibold text-foreground mb-0.5">Derived Validations</div>
                        <div class="text-muted-foreground">{{ validationReport.summary_level.derived.checks.filter(c => c.passed).length }} / {{ validationReport.summary_level.derived.checks.length }} checks passed</div>
                      </div>
                      <div v-if="!summaryChecksRun">
                        <div class="font-semibold text-foreground mb-0.5">Statement Summary</div>
                        <div class="text-muted-foreground">Totals not printed (nothing to reconcile)</div>
                      </div>
                    </div>
                  </TooltipContent>
                </Tooltip>
              </TooltipProvider>

              <!-- Transaction Badge -->
              <TooltipProvider>
                <Tooltip>
                  <TooltipTrigger class="cursor-help flex items-center gap-1.5 font-semibold text-sm w-fit"
                       :class="{
                         'text-muted-foreground': !validationReport.row_level?.checked_rows,
                         'text-emerald-500': validationReport.row_level?.checked_rows > 0 && validationReport.row_level?.passed,
                         'text-destructive': validationReport.row_level?.checked_rows > 0 && !validationReport.row_level?.passed
                       }">
                    <CheckCircle2 v-if="validationReport.row_level?.passed && validationReport.row_level?.checked_rows > 0" class="w-4 h-4" />
                    <XCircle v-else-if="!validationReport.row_level?.passed" class="w-4 h-4" />
                    <MinusCircle v-else class="w-4 h-4 opacity-70" />
                    <span>Transactions</span>
                  </TooltipTrigger>
                  <TooltipContent side="bottom" class="p-3 max-w-sm">
                    <div class="space-y-1 text-sm">
                      <div class="font-semibold text-foreground mb-0.5">Running Transactions</div>
                      <div class="text-muted-foreground" v-if="validationReport.row_level?.checked_rows > 0">
                        {{ (validationReport.row_level?.checked_rows || 0) - (validationReport.row_level?.failed_rows?.length || 0) }} / {{ validationReport.row_level?.checked_rows || 0 }} txns passed
                      </div>
                      <div class="text-muted-foreground" v-else>
                        Balances not printed ({{ totalTxns }} txns extracted)
                      </div>
                    </div>
                  </TooltipContent>
                </Tooltip>
              </TooltipProvider>
            </div>

            <div v-else class="text-sm font-medium text-muted-foreground">-</div>
          </div>
          <div class="flex flex-col text-right">
            <span class="text-[10px] text-muted-foreground uppercase tracking-wider font-semibold mb-0.5">Parse Time</span>
            <span class="font-mono text-sm">{{ parseTime }}s</span>
          </div>
        </Card>
      </div>

      
      <!-- Credit Card Results Table -->
      <div v-if="ccStatement" class="space-y-6">
        
        <!-- Standardized Header -->
        <StatementHeader 
          :validationStatus="validationReport?.overall"
          :customerName="ccStatement.profile?.holders?.holder?.[0]?.name || 'Customer'"
          :institutionName="ccStatement.xfina?.institutionName || 'Credit Card'"
          statementType="Credit Card"
          :accountNumber="ccStatement.maskedAccNumber || ''"
          :statementDetails="[
            ...(ccStatement.summary?.xfina?.cardProduct ? [{ label: 'Product', value: ccStatement.summary.xfina.cardProduct }] : []),
            ...(ccStatement.transactions?.startDate ? [{ label: 'From', value: formatDate(ccStatement.transactions.startDate), derived: ccStatement.transactions?.xfina?.startDateDerived }] : []),
            ...(ccStatement.transactions?.endDate ? [{ label: 'To', value: formatDate(ccStatement.transactions.endDate), derived: ccStatement.transactions?.xfina?.endDateDerived }] : []),
            ...(ccStatement.xfina?.generatedDate ? [{ label: 'Generated', value: formatDateTime(ccStatement.xfina.generatedDate, 'xfina.generatedDate', ccStatement.xfina?.dateOnlyPaths), derived: ccStatement.xfina?.generatedDateDerived }] : []),
            ...(ccStatement.summary?.dueDate ? [{ label: 'Due Date', value: formatDate(ccStatement.summary.dueDate) }] : [])
          ]"
        />

        <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          <Card class="bg-card text-card-foreground shadow-sm">
            <CardHeader class="pb-2">
              <CardTitle class="text-sm text-muted-foreground font-semibold uppercase tracking-wider">Account Summary</CardTitle>
            </CardHeader>
            <CardContent v-if="ccStatement.summary">
              <div class="grid grid-cols-[1fr_auto_auto] gap-x-4 gap-y-2 items-center">
                <div class="col-span-3 flex justify-between items-center mb-1 border-b pb-2">
                  <span class="text-sm font-medium">Opening Balance</span>
                  <span class="font-bold font-mono text-lg text-primary">{{ formatCurrency(ccStatement.summary.xfina?.openingBalance) }}</span>
                </div>
                
                <span class="text-sm text-muted-foreground">Payments</span>
                <div class="justify-self-end">
                  <TooltipProvider>
                    <Tooltip>
                      <TooltipTrigger class="text-xs text-muted-foreground bg-muted/50 px-1.5 rounded cursor-help font-mono border border-border/50">
                        {{ ccStatement.transactions?.transaction?.filter(t => t.txnType === 'CREDIT').length || 0 }}
                      </TooltipTrigger>
                      <TooltipContent>
                        <p>Number of payments</p>
                      </TooltipContent>
                    </Tooltip>
                  </TooltipProvider>
                </div>
                <span class="font-medium font-mono text-emerald-500 text-right">+ {{ formatCurrency(ccStatement.summary.xfina?.paymentCredit) }}</span>

                <div v-if="ccStatement.summary.xfina?.ownerCreditBreakdown && Object.keys(ccStatement.summary.xfina.ownerCreditBreakdown).length > 1" class="col-span-3 pl-4 border-l-2 border-muted space-y-1 my-1">
                  <div v-for="(amount, owner) in ccStatement.summary.xfina.ownerCreditBreakdown" :key="owner" class="flex justify-between items-center">
                    <span class="text-sm text-muted-foreground truncate mr-2">{{ owner }}</span>
                    <span class="font-medium font-mono text-sm text-emerald-500">+ {{ formatCurrency(amount) }}</span>
                  </div>
                </div>
                
                <span class="text-sm text-muted-foreground">Purchases</span>
                <div class="justify-self-end">
                  <TooltipProvider>
                    <Tooltip>
                      <TooltipTrigger class="text-xs text-muted-foreground bg-muted/50 px-1.5 rounded cursor-help font-mono border border-border/50">
                        {{ ccStatement.transactions?.transaction?.filter(t => t.txnType === 'DEBIT').length || 0 }}
                      </TooltipTrigger>
                      <TooltipContent>
                        <p>Number of purchases</p>
                      </TooltipContent>
                    </Tooltip>
                  </TooltipProvider>
                </div>
                <span class="font-medium font-mono text-foreground text-right">{{ formatCurrency(ccStatement.summary.xfina?.purchasesDebits) }}</span>
                
                <div v-if="ccStatement.summary.xfina?.ownerDebitBreakdown && Object.keys(ccStatement.summary.xfina.ownerDebitBreakdown).length > 1" class="col-span-3 pl-4 border-l-2 border-muted space-y-1 my-1">
                  <div v-for="(amount, owner) in ccStatement.summary.xfina.ownerDebitBreakdown" :key="owner" class="flex justify-between items-center">
                    <span class="text-sm text-muted-foreground truncate mr-2">{{ owner }}</span>
                    <span class="font-medium font-mono text-sm text-foreground">{{ formatCurrency(amount) }}</span>
                  </div>
                </div>

                <div v-if="ccStatement.summary.financeCharges > 0" class="col-span-3 flex justify-between items-center">
                  <span class="text-sm text-muted-foreground">Finance Charges</span>
                  <span class="font-medium font-mono text-foreground">{{ formatCurrency(ccStatement.summary.financeCharges) }}</span>
                </div>
                
                <div class="col-span-3 flex justify-between items-center mt-1 border-t pt-2">
                  <span class="text-sm font-medium">Total Dues</span>
                  <span class="font-bold font-mono text-lg text-primary">{{ formatCurrency(ccStatement.summary.totalDueAmount) }}</span>
                </div>
                
                <div class="col-span-3 flex justify-between items-center mt-1">
                  <span class="text-xs text-muted-foreground">Min Amount Due</span>
                  <span class="font-medium font-mono text-xs">{{ formatCurrency(ccStatement.summary.minDueAmount) }}</span>
                </div>
              </div>
            </CardContent>
          </Card>

          <Card class="bg-card text-card-foreground shadow-sm">
            <CardHeader class="pb-2">
              <CardTitle class="text-sm text-muted-foreground font-semibold uppercase tracking-wider">Credit Limits</CardTitle>
            </CardHeader>
            <CardContent>
              <div class="space-y-2">
                <div class="flex justify-between items-center"><span class="text-sm text-muted-foreground">Credit Limit</span><span class="font-medium font-mono">{{ formatCurrency(ccStatement.summary?.creditLimit) }}</span></div>
                <div class="flex justify-between items-center"><span class="text-sm text-muted-foreground">Available Limit</span><span class="font-medium font-mono">{{ formatCurrency(ccStatement.summary?.availableCredit) }}</span></div>
                <div class="flex justify-between items-center"><span class="text-sm text-muted-foreground">Cash Limit</span><span class="font-medium font-mono">{{ formatCurrency(ccStatement.summary?.cashLimit) }}</span></div>
              </div>
            </CardContent>
          </Card>

          <Card v-if="hasRewards(ccStatement)" class="bg-card text-card-foreground shadow-sm">
            <CardHeader class="pb-2">
              <CardTitle class="text-sm text-muted-foreground font-semibold uppercase tracking-wider">Rewards Summary</CardTitle>
            </CardHeader>
            <CardContent>
              <div class="space-y-2">
                <div v-if="ccStatement.summary.xfina.rewardPointsSummary.openingBalance !== 0 || ccStatement.summary.xfina.rewardPointsSummary.closingBalance !== 0" class="flex justify-between items-center mb-2 border-b pb-2"><span class="text-sm font-medium">Opening Balance</span><span class="font-bold font-mono text-lg text-primary">{{ formatNumber(ccStatement.summary.xfina.rewardPointsSummary.openingBalance) }}</span></div>
                
                <div class="flex justify-between items-center"><span class="text-sm text-muted-foreground">Earned</span><span class="font-medium font-mono text-emerald-500">+{{ formatNumber(ccStatement.summary.xfina.rewardPointsSummary.earned) }}</span></div>
                
                <div v-if="(ccStatement.summary.xfina.rewardPrograms && ccStatement.summary.xfina.rewardPrograms.length > 0) || ccStatement.summary.xfina.rewardPointsSummary.earnedUnaccounted" class="pl-4 border-l-2 border-muted space-y-1 my-1">
                  <div class="flex justify-between items-center">
                    <span class="text-sm text-muted-foreground truncate mr-2">Rewards</span>
                    <span class="font-medium font-mono text-sm text-emerald-500">+{{ formatNumber(ccStatement.summary.xfina.rewardPointsSummary.defaultRewards) }}</span>
                  </div>
                  <div v-for="(prog, idx) in ccStatement.summary.xfina.rewardPrograms" :key="idx" class="flex justify-between items-center">
                    <span class="text-sm text-muted-foreground truncate mr-2" :title="prog.program">{{ prog.program }}</span>
                    <span class="font-medium font-mono text-sm text-emerald-500">+{{ formatNumber(prog.bonusPoints) }}</span>
                  </div>
                  <!-- What the statement credits without itemising it, e.g. a
                       balance carried over from a replaced card. -->
                  <div v-if="ccStatement.summary.xfina.rewardPointsSummary.earnedUnaccounted" class="flex justify-between items-center" title="Earned that the transactions and bonus programs do not account for">
                    <span class="text-sm text-muted-foreground truncate mr-2">Unaccounted<span class="ml-1 text-[10px] font-semibold text-muted-foreground/70 bg-muted/40 rounded px-1 py-0.5 align-middle">derived</span></span>
                    <span class="font-medium font-mono text-sm text-amber-500">{{ ccStatement.summary.xfina.rewardPointsSummary.earnedUnaccounted > 0 ? '+' : '' }}{{ formatNumber(ccStatement.summary.xfina.rewardPointsSummary.earnedUnaccounted) }}</span>
                  </div>
                </div>

                <div v-if="ccStatement.summary.xfina.rewardPointsSummary.disbursed > 0" class="flex justify-between items-center"><span class="text-sm text-muted-foreground">Disbursed</span><span class="font-medium font-mono text-foreground">{{ formatNumber(ccStatement.summary.xfina.rewardPointsSummary.disbursed) }}</span></div>
                <div v-if="ccStatement.summary.xfina.rewardPointsSummary.adjustedLapsed > 0" class="flex justify-between items-center"><span class="text-sm text-muted-foreground">Adjusted / Lapsed</span><span class="font-medium font-mono text-foreground">{{ formatNumber(ccStatement.summary.xfina.rewardPointsSummary.adjustedLapsed) }}</span></div>
                <!-- The transaction reversals Adjusted / Lapsed includes. -->
                <div v-if="ccStatement.summary.xfina.rewardPointsSummary.adjustedLapsed > 0 && reversedPoints(ccStatement)" class="pl-4 border-l-2 border-muted space-y-1 my-1">
                  <div class="flex justify-between items-center">
                    <span class="text-sm text-muted-foreground truncate mr-2">Reversals</span>
                    <span class="font-medium font-mono text-sm text-foreground">{{ formatNumber(reversedPoints(ccStatement)) }}</span>
                  </div>
                </div>
                
                <div v-if="ccStatement.summary.xfina.rewardPointsSummary.openingBalance !== 0 || ccStatement.summary.xfina.rewardPointsSummary.closingBalance !== 0" class="flex justify-between items-center mt-2 border-t pt-2"><span class="text-sm font-medium">Closing Balance</span><span class="font-bold font-mono text-lg text-primary">{{ formatNumber(ccStatement.summary.xfina.rewardPointsSummary.closingBalance) }}</span></div>
                <div v-if="ccStatement.summary.xfina.rewardPointsSummary.expiringIn30Days" class="flex justify-between items-center text-rose-500"><span class="text-xs">Expiring (30d)</span><span class="font-medium font-mono text-xs">{{ formatNumber(ccStatement.summary.xfina.rewardPointsSummary.expiringIn30Days) }}</span></div>
                <div v-if="ccStatement.summary.xfina.rewardPointsSummary.expiringIn60Days" class="flex justify-between items-center text-rose-500"><span class="text-xs">Expiring (60d)</span><span class="font-medium font-mono text-xs">{{ formatNumber(ccStatement.summary.xfina.rewardPointsSummary.expiringIn60Days) }}</span></div>
              </div>
            </CardContent>
          </Card>
        </div>

        <Accordion type="single" collapsible class="w-full">
          <AccordionItem value="transactions" class="border rounded-lg bg-card text-card-foreground shadow-sm overflow-hidden" :disabled="!ccStatement.transactions?.transaction?.length">
            <AccordionTrigger class="group hover:no-underline px-4 py-4 data-[state=open]:border-b border-border">
              <span class="font-medium text-foreground text-lg text-left w-full pr-4">Transactions</span>
              <template #icon>
                <div class="flex items-center gap-1.5 text-xs font-mono bg-primary/10 text-primary pl-2.5 pr-2 py-1.5 rounded shrink-0 ml-2">
                  <span>{{ ccStatement.transactions?.transaction?.length || 0 }} {{ ccStatement.transactions?.transaction?.length === 1 ? 'Txn' : 'Txns' }}</span>
                  <ChevronDown v-if="ccStatement.transactions?.transaction?.length" class="h-4 w-4 transition-transform duration-200 group-data-[state=open]:rotate-180" />
                </div>
              </template>
            </AccordionTrigger>
            <AccordionContent class="p-4">
              <div class="rounded-md border border-border overflow-x-auto">
              <Table>
                <TableHeader class="bg-muted/50">
                  <TableRow class="hover:bg-transparent">
                    <TableHead class="w-[150px] text-muted-foreground whitespace-nowrap">Date</TableHead>
                    <TableHead class="text-muted-foreground whitespace-nowrap">Description</TableHead>
                    <TableHead class="w-[150px] text-muted-foreground whitespace-nowrap">Card Name</TableHead>
                    <TableHead class="w-[140px] text-right text-muted-foreground whitespace-nowrap">Amount</TableHead>
                    <TableHead class="w-[120px] text-right text-muted-foreground whitespace-nowrap">Rewards</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  <TableRow v-for="(txn, idx) in ccStatement.transactions?.transaction" :key="idx" class="hover:bg-muted/50 transition-colors">
                    <TableCell class="text-foreground whitespace-nowrap">{{ formatDateTime(txn.txnDate, 'transactions.transaction.txnDate', ccStatement.xfina?.dateOnlyPaths) }}</TableCell>
                    <TableCell class="text-foreground text-sm">
                      <span v-if="txn.xfina?.category" class="mr-2 px-1.5 py-0.5 rounded text-[10px] font-bold bg-muted text-muted-foreground">{{ txn.xfina.category }}</span>
                      {{ txn.narration }}
                    </TableCell>
                    <TableCell class="text-foreground text-xs text-muted-foreground whitespace-nowrap">{{ txn.xfina?.owner }}</TableCell>
                    <TableCell class="text-right font-mono whitespace-nowrap" :class="{'text-emerald-500': txn.txnType === 'CREDIT', 'text-foreground': txn.txnType !== 'CREDIT'}">
                      <div class="inline-flex items-baseline justify-end">
                        <span v-if="txn.txnType === 'CREDIT'">+</span>
                        <span>{{ formatCurrency(txn.amount) }}</span>
                      </div>
                    </TableCell>
                    <TableCell class="text-right font-mono text-emerald-500">{{ txn.xfina?.rewardPoints > 0 ? '+' + txn.xfina.rewardPoints : (txn.xfina?.rewardPoints || '') }}</TableCell>
                  </TableRow>
                </TableBody>
              </Table>
            </div>
            </AccordionContent>
          </AccordionItem>
        </Accordion>
      </div>

      <!-- Results Table -->
      <div v-if="mfStatement" class="space-y-6">
        
        <!-- Standardized Header -->
        <StatementHeader 
          v-if="mfStatement.profile?.holders?.holder?.length"
          :customerName="mfStatement.profile.holders.holder[0].name || 'Investor'"
          :institutionName="result?.institution || 'Mutual Funds'"
          statementType="Mutual Funds"
          :accountNumber="mfStatement.profile.holders.holder[0].pan || ''"
          :statementDetails="[
            ...(mfStatement.transactions?.startDate ? [{ label: 'From', value: formatDate(mfStatement.transactions.startDate) }] : []),
            ...(mfStatement.transactions?.endDate ? [{ label: 'To', value: formatDate(mfStatement.transactions.endDate) }] : []),
            ...(mfStatement.xfina?.generatedDate ? [{ label: 'Generated', value: formatDateTime(mfStatement.xfina.generatedDate, 'xfina.generatedDate', mfStatement.xfina?.dateOnlyPaths), derived: mfStatement.xfina?.generatedDateDerived }] : [])
          ]"
        />

        <div class="grid grid-cols-1 gap-4" v-if="mfStatement.summary?.investmentValue !== undefined || mfStatement.summary?.currentValue !== undefined">
          <Card class="bg-card text-card-foreground shadow-sm">
            <CardHeader class="pb-2 border-b mb-3">
              <CardTitle class="text-sm text-muted-foreground font-semibold uppercase tracking-wider">Portfolio Summary</CardTitle>
            </CardHeader>
            <CardContent>
              <div class="grid grid-cols-2 md:grid-cols-4 gap-6">
                <div class="flex flex-col">
                  <span class="text-xs text-muted-foreground mb-1">Total Assets</span>
                  <span class="font-medium font-mono text-xl text-foreground">{{ camsGroupedAssets.length || 0 }}</span>
                </div>
                <div class="flex flex-col">
                  <span class="text-xs text-muted-foreground mb-1">Total Invested</span>
                  <span class="font-medium font-mono text-xl">{{ formatCurrency(mfStatement.summary?.investmentValue) }}</span>
                </div>
                <div class="flex flex-col">
                  <span class="text-xs text-muted-foreground mb-1">Market Value</span>
                  <span class="font-medium font-mono text-xl text-primary">{{ formatCurrency(mfStatement.summary?.currentValue) }}</span>
                </div>
                <div class="flex flex-col">
                  <span class="text-xs text-muted-foreground mb-1">Unrealized P&L</span>
                  <span class="font-medium font-mono text-xl" 
                        :class="(mfStatement.summary?.currentValue || 0) > (mfStatement.summary?.investmentValue || 0) ? 'text-emerald-500' : ((mfStatement.summary?.currentValue || 0) < (mfStatement.summary?.investmentValue || 0) ? 'text-rose-500' : 'text-foreground')">
                    {{ (mfStatement.summary?.currentValue || 0) > (mfStatement.summary?.investmentValue || 0) ? '+ ' : '' }}{{ formatCurrency((mfStatement.summary?.currentValue || 0) - (mfStatement.summary?.investmentValue || 0)) }}
                  </span>
                </div>
              </div>
            </CardContent>
          </Card>
        </div>

        <Accordion type="multiple" class="w-full space-y-4">
           <AccordionItem 
             v-for="(asset, index) in camsGroupedAssets" 
             :key="index" 
             :value="`item-${index}`"
             class="border rounded-lg bg-card text-card-foreground shadow-sm overflow-hidden group/item"
             :disabled="!asset.transactions?.length"
           >
               <div class="px-4 py-4 flex flex-col items-start w-full gap-3 border-b border-transparent transition-colors group-data-[state=open]/item:border-border group-data-[state=open]/item:border-b">
                 <div class="flex flex-col items-start w-full gap-3">
                   <!-- Top Row: Chevron, Name, Tags, Txn Pill -->
                   <div class="grid grid-cols-[auto_1fr_auto] items-start gap-4 w-full">
                     <span class="text-xs font-medium font-mono bg-muted/30 border border-primary/20 rounded px-2 py-0.5 text-primary shadow-sm shrink-0" v-if="asset.isin">{{ asset.isin }}</span>
                     
                     <div class="flex flex-wrap items-center gap-2 min-w-0">
                       <span class="font-medium text-foreground text-left text-base lg:text-lg leading-tight break-words">{{ asset.name }}</span>
                       <span class="text-xs font-medium font-mono bg-muted/30 border border-primary/20 rounded px-2 py-0.5 text-primary shadow-sm shrink-0" v-if="asset.symbol">{{ asset.symbol }}</span>
                     </div>
                    <AccordionTrigger class="py-1.5 flex-none font-mono text-xs font-normal hover:no-underline justify-end gap-1.5 bg-primary/10 text-primary hover:bg-primary/20 transition-colors pl-2.5 pr-2 rounded shrink-0 group w-auto" :disabled="!asset.transactions?.length">
                       <span>{{ asset.transactions?.length || 0 }} {{ asset.transactions?.length === 1 ? 'Txn' : 'Txns' }}</span>
                       <ChevronDown v-if="asset.transactions?.length" class="h-4 w-4 shrink-0 transition-transform duration-200 group-data-[state=open]:rotate-180" />
                       <template #icon><span class="hidden"></span></template>
                     </AccordionTrigger>
                   </div>
                   
                   <!-- Metadata / Account Details -->
                   <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-x-6 gap-y-3 w-full px-1 pt-1 pb-2" v-if="asset.folioNo || asset.registrar || asset.kyc || asset.advisor || (asset.nominees && asset.nominees.length)">
                     <div class="flex flex-col" v-if="asset.folioNo">
                       <span class="text-[10px] text-muted-foreground uppercase tracking-wider font-semibold mb-0.5">Folio No</span>
                       <span class="font-medium font-mono text-xs">{{ asset.folioNo }}</span>
                     </div>
                     <div class="flex flex-col" v-if="asset.registrar">
                       <span class="text-[10px] text-muted-foreground uppercase tracking-wider font-semibold mb-0.5">Registrar</span>
                       <span class="font-medium text-xs truncate" :title="asset.registrar">{{ asset.registrar }}</span>
                     </div>
                     <div class="flex flex-col" v-if="asset.kyc || asset.panKyc">
                       <span class="text-[10px] text-muted-foreground uppercase tracking-wider font-semibold mb-0.5">KYC / PAN</span>
                       <span class="font-medium text-xs">{{ asset.kyc || '-' }} / {{ asset.panKyc || '-' }}</span>
                     </div>
                     <div class="flex flex-col" v-if="asset.advisor">
                       <span class="text-[10px] text-muted-foreground uppercase tracking-wider font-semibold mb-0.5">Advisor</span>
                       <span class="font-medium text-xs truncate" :title="asset.advisor">{{ asset.advisor }}</span>
                     </div>
                     <div class="flex flex-col" v-if="asset.nominees && asset.nominees.length">
                       <span class="text-[10px] text-muted-foreground uppercase tracking-wider font-semibold mb-0.5">Nominees</span>
                       <span class="font-medium text-xs truncate" :title="asset.nominees.join(', ')">{{ asset.nominees.join(', ') }}</span>
                     </div>
                   </div>
                   
                   <!-- 2-Column Blocks -->
                   <div class="flex flex-col lg:flex-row gap-3 w-full">
                     <!-- Box 1: Asset Summary (Opening, Buys, Sells, Closing, NAV, NAV Date) -->
                     <div class="flex items-center justify-between text-xs bg-muted/20 border border-border rounded-md px-3.5 py-2.5 gap-3 flex-1 overflow-x-auto [&>.w-px:last-child]:hidden">
                       <div class="flex flex-col items-end shrink-0">
                         <span class="text-[10px] text-muted-foreground uppercase tracking-wider mb-0.5">Opening</span>
                         <span class="font-mono font-bold text-foreground text-sm text-right">{{ formatUnits(asset.openingBalance || 0) }}</span>
                       </div>
                       <div class="w-px h-8 bg-border/60"></div>
                       <div class="flex flex-col items-end shrink-0" v-if="asset.periodBuyUnits || asset.periodBuyCount">
                         <div class="flex items-center gap-1.5 mb-0.5">
                           <span v-if="asset.periodBuyCount" class="text-xs text-muted-foreground bg-muted/50 px-1.5 py-0.5 rounded font-mono border border-border/50">{{ asset.periodBuyCount }}</span>
                           <span class="text-[10px] text-muted-foreground uppercase tracking-wider">Buys</span>
                         </div>
                         <span class="font-mono font-bold text-foreground text-sm text-right">{{ formatUnits(asset.periodBuyUnits || 0) }}</span>
                       </div>
                       <div class="w-px h-8 bg-border/60" v-if="asset.periodBuyUnits || asset.periodBuyCount"></div>
                       <div class="flex flex-col items-end shrink-0" v-if="asset.periodSellUnits || asset.periodSellCount">
                         <div class="flex items-center gap-1.5 mb-0.5">
                           <span v-if="asset.periodSellCount" class="text-xs text-muted-foreground bg-muted/50 px-1.5 py-0.5 rounded font-mono border border-border/50">{{ asset.periodSellCount }}</span>
                           <span class="text-[10px] text-muted-foreground uppercase tracking-wider">Sells</span>
                         </div>
                         <span class="font-mono font-bold text-foreground text-sm text-right">{{ formatUnits(asset.periodSellUnits || 0) }}</span>
                       </div>
                       <div class="w-px h-8 bg-border/60" v-if="asset.periodSellUnits || asset.periodSellCount"></div>
                       <div class="flex flex-col items-end shrink-0">
                         <span class="text-[10px] text-muted-foreground uppercase tracking-wider mb-0.5">Closing</span>
                         <span class="font-mono font-bold text-primary text-sm text-right">{{ formatUnits(asset.closingBalance || 0) }}</span>
                       </div>
                       <div class="w-px h-8 bg-border/60"></div>
                       <div class="flex flex-col items-end shrink-0">
                         <span class="text-[10px] text-muted-foreground uppercase tracking-wider mb-0.5">NAV</span>
                         <span class="font-mono font-bold text-foreground text-sm text-right">{{ formatCurrency(asset.nav) }}</span>
                       </div>
                       <div class="w-px h-8 bg-border/60" v-if="asset.navDate"></div>
                       <div class="flex flex-col items-end shrink-0" v-if="asset.navDate">
                         <span class="text-[10px] text-muted-foreground uppercase tracking-wider mb-0.5">NAV Date</span>
                         <span class="font-mono font-medium text-foreground text-sm text-right">{{ formatDate(asset.navDate) }}</span>
                       </div>
                     </div>
                     
                     <div class="flex items-center text-right bg-muted/20 border border-border rounded-md px-3.5 py-2.5 shrink-0 gap-3">
                       <div class="flex flex-col items-end w-[130px] shrink-0">
                         <span class="text-[10px] text-muted-foreground uppercase tracking-wider mb-0.5">Total Invested</span>
                         <span class="text-sm font-medium font-mono text-foreground">{{ formatCurrency(asset.totalInvested) }}</span>
                       </div>
                       <div class="w-px h-8 bg-border/60"></div>
                       <div class="flex flex-col items-end w-[130px] shrink-0">
                         <span class="text-[10px] text-muted-foreground uppercase tracking-wider mb-0.5">Market Value</span>
                         <span class="text-sm font-bold font-mono text-primary">{{ formatCurrency(asset.marketValue) }}</span>
                       </div>
                       <div class="w-px h-8 bg-border/60"></div>
                       <div class="flex flex-col items-end w-[130px] shrink-0">
                         <span class="text-[10px] text-muted-foreground uppercase tracking-wider mb-0.5">Unrealized P&L</span>
                         <span class="font-mono font-bold text-sm text-foreground">
                           {{ asset.unrealizedPl > 0 ? '+ ' : '' }}{{ formatCurrency(asset.unrealizedPl) }}
                         </span>
                       </div>
                     </div>
                   </div>
                 </div>
                  </div>
                <AccordionContent>
                 <div class="rounded-md border border-border mt-2 overflow-x-auto">
                   <Table>
                     <TableHeader class="bg-muted/50">
                       <TableRow class="hover:bg-transparent">
                         <TableHead class="w-[150px] text-muted-foreground whitespace-nowrap">Date</TableHead>
                         <TableHead class="w-[100px] text-muted-foreground whitespace-nowrap">Type</TableHead>
                         <TableHead class="text-muted-foreground whitespace-nowrap">Description</TableHead>
                         <TableHead class="w-[140px] text-right text-muted-foreground whitespace-nowrap">
                           <div class="flex items-center justify-end gap-1.5">
                             <span>Amount</span>
                             <TooltipProvider>
                               <Tooltip>
                                 <TooltipTrigger class="cursor-help">
                                   <HelpCircle class="h-3.5 w-3.5 text-muted-foreground" />
                                 </TooltipTrigger>
                                 <TooltipContent>
                                   <p class="max-w-[200px] text-xs font-normal whitespace-normal text-left">Invested (including the fee), if redeemed it is excluding the fee</p>
                                 </TooltipContent>
                               </Tooltip>
                             </TooltipProvider>
                           </div>
                         </TableHead>
                         <TableHead class="w-[120px] text-right text-muted-foreground whitespace-nowrap">Units</TableHead>
                         <TableHead class="w-[120px] text-right text-muted-foreground whitespace-nowrap">Price</TableHead>
                         <TableHead class="w-[120px] text-right text-muted-foreground whitespace-nowrap">Fees</TableHead>
                         <TableHead class="w-[140px] text-right text-muted-foreground whitespace-nowrap">Balance</TableHead>
                       </TableRow>
                     </TableHeader>
                     <TableBody>
                       <TableRow v-for="(txn, idx) in asset.transactions" :key="idx" class="hover:bg-muted/50 transition-colors">
                         <TableCell class="text-foreground whitespace-nowrap">{{ formatDate(txn.orderDate) }}</TableCell>
                         <TableCell class="text-foreground">
                            <span class="font-medium text-xs px-2 py-1 rounded" :class="{'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400': txn.type === 'BUY', 'bg-rose-100 text-rose-700 dark:bg-rose-900/30 dark:text-rose-400': txn.type === 'SELL'}">
                              {{ txn.type || '-' }}
                            </span>
                          </TableCell>
                         <TableCell class="text-foreground text-xs whitespace-pre-line">{{ txn.narration || '-' }}</TableCell>
                         <TableCell class="text-right font-mono text-foreground whitespace-nowrap">
                            {{ formatCurrency(txn.type === 'BUY' ? (Number(txn.amount) + Number(txn.xfina?.fees || 0)) : Number(txn.amount)) }}
                          </TableCell>
                         <TableCell class="text-right font-mono text-foreground whitespace-nowrap">{{ formatUnits(txn.xfina?.units) }}</TableCell>
                         <TableCell class="text-right font-mono text-foreground whitespace-nowrap">{{ formatCurrency(txn.nav) }}</TableCell>
                         <TableCell class="text-right font-mono text-foreground whitespace-nowrap">{{ txn.xfina?.fees > 0 ? formatCurrency(txn.xfina.fees) : '' }}</TableCell>
                         <TableCell class="text-right font-mono text-foreground whitespace-nowrap">{{ txn.closingUnits !== undefined && txn.closingUnits !== null ? formatUnits(txn.closingUnits) : '-' }}</TableCell>
                       </TableRow>
                     </TableBody>
                   </Table>
                 </div>
               </AccordionContent>
             </AccordionItem>
            </Accordion>
      </div>

      <!-- Bank Statement Results Table -->
      <div v-if="bankStatement" class="space-y-6">
        
        <!-- Standardized Header -->
        <StatementHeader
          :validationStatus="validationReport?.overall"
          :customerName="bankStatement.profile?.holders?.holder?.[0]?.name || 'Customer'"
          :address="bankStatement.profile?.holders?.holder?.[0]?.address || ''"
          :customerId="bankStatement.profile?.holders?.holder?.[0]?.xfina?.customerId || ''"
          :institutionName="bankStatement.xfina?.institutionName || 'Bank'"
          statementType="Bank Account"
          :accountNumber="bankStatement.maskedAccNumber || ''"
          :statementDetails="[
            ...(bankStatement.transactions?.startDate ? [{ label: 'From', value: formatDate(bankStatement.transactions.startDate) }] : []),
            ...(bankStatement.transactions?.endDate ? [{ label: 'To', value: formatDate(bankStatement.transactions.endDate) }] : []),
            ...(bankStatement.xfina?.generatedDate ? [{ label: 'Generated', value: formatDateTime(bankStatement.xfina.generatedDate, 'xfina.generatedDate', bankStatement.xfina?.dateOnlyPaths), derived: bankStatement.xfina?.generatedDateDerived }] : [])
          ]"
        />

        <div class="grid grid-cols-1 lg:grid-cols-3 gap-4">
          <Card class="bg-card text-card-foreground shadow-sm h-full" v-if="bankStatement.summary?.xfina?.openingBalance !== null && bankStatement.summary?.xfina?.openingBalance !== undefined">
            <CardHeader class="pb-2">
              <CardTitle class="text-sm text-muted-foreground font-semibold uppercase tracking-wider">Transaction Summary</CardTitle>
            </CardHeader>
            <CardContent>
              <div class="grid grid-cols-[1fr_auto_auto] gap-x-4 gap-y-2 items-center">
                <div class="col-span-3 flex justify-between items-center mb-1 border-b pb-2">
                  <span class="text-sm font-medium">Opening Balance</span>
                  <span class="font-bold font-mono text-lg text-foreground">{{ formatCurrency(bankStatement.summary?.xfina?.openingBalance) }}</span>
                </div>
                
                <span class="text-sm text-muted-foreground">Deposits</span>
                <div class="justify-self-end">
                  <TooltipProvider>
                    <Tooltip>
                      <TooltipTrigger class="text-xs text-muted-foreground bg-muted/50 px-1.5 rounded cursor-help font-mono border border-border/50">
                        {{ bankStatement.transactions?.transaction?.filter(t => t.type === 'CREDIT').length || 0 }}
                      </TooltipTrigger>
                      <TooltipContent>
                        <p>Number of deposits</p>
                      </TooltipContent>
                    </Tooltip>
                  </TooltipProvider>
                </div>
                <span class="font-medium font-mono text-emerald-500 text-right">+ {{ formatCurrency(bankStatement.transactions?.transaction?.filter(t => t.type === 'CREDIT').reduce((s, t) => s + Number(t.amount || 0), 0) || 0) }}</span>
                
                <span class="text-sm text-muted-foreground">Withdrawals</span>
                <div class="justify-self-end">
                  <TooltipProvider>
                    <Tooltip>
                      <TooltipTrigger class="text-xs text-muted-foreground bg-muted/50 px-1.5 rounded cursor-help font-mono border border-border/50">
                        {{ bankStatement.transactions?.transaction?.filter(t => t.type === 'DEBIT').length || 0 }}
                      </TooltipTrigger>
                      <TooltipContent>
                        <p>Number of withdrawals</p>
                      </TooltipContent>
                    </Tooltip>
                  </TooltipProvider>
                </div>
                <span class="font-medium font-mono text-foreground text-right">{{ formatCurrency(bankStatement.transactions?.transaction?.filter(t => t.type === 'DEBIT').reduce((s, t) => s + Number(t.amount || 0), 0) || 0) }}</span>
                
                <div class="col-span-3 flex justify-between items-center mt-1 border-t pt-2">
                  <span class="text-sm font-medium">Closing Balance</span>
                  <span class="font-bold font-mono text-lg text-primary">{{ formatCurrency(bankStatement.summary?.currentBalance) }}</span>
                </div>
              </div>
            </CardContent>
          </Card>

          <!-- Account Details Card -->
          <Card class="bg-card text-card-foreground shadow-sm lg:col-span-2" v-if="bankStatement.summary?.xfina?.accountProduct || bankStatement.profile?.holders?.holder?.[0]?.nominee || bankStatement.summary?.branch || bankStatement.summary?.ifscCode || bankStatement.summary?.micrCode || bankStatement.summary?.openingDate">
            <CardHeader class="pb-2">
              <CardTitle class="text-sm text-muted-foreground font-semibold uppercase tracking-wider">Account Details</CardTitle>
            </CardHeader>
            <CardContent>
               <div class="grid grid-cols-1 sm:grid-cols-3 gap-x-8 gap-y-4 text-sm mt-1">
                 <div class="flex flex-col" v-if="bankStatement.summary?.branch">
                   <span class="text-muted-foreground text-xs uppercase tracking-wider font-semibold">Branch</span>
                   <span class="font-medium mt-0.5">{{ bankStatement.summary?.branch }}</span>
                 </div>
                 <div class="flex flex-col" v-if="bankStatement.summary?.ifscCode">
                   <span class="text-muted-foreground text-xs uppercase tracking-wider font-semibold">IFSC Code</span>
                   <span class="font-medium font-mono mt-0.5">{{ bankStatement.summary?.ifscCode }}</span>
                 </div>
                 <div class="flex flex-col" v-if="bankStatement.summary?.micrCode">
                   <span class="text-muted-foreground text-xs uppercase tracking-wider font-semibold">MICR Code</span>
                   <span class="font-medium font-mono mt-0.5">{{ bankStatement.summary?.micrCode }}</span>
                 </div>
                 <div class="flex flex-col" v-if="bankStatement.summary?.xfina?.accountProduct">
                   <span class="text-muted-foreground text-xs uppercase tracking-wider font-semibold">Product</span>
                   <span class="font-medium mt-0.5">{{ bankStatement.summary?.xfina?.accountProduct }}</span>
                 </div>
                 <div class="flex flex-col" v-if="bankStatement.summary?.openingDate">
                   <span class="text-muted-foreground text-xs uppercase tracking-wider font-semibold">Opening Date</span>
                   <span class="font-medium mt-0.5">{{ formatDate(bankStatement.summary?.openingDate) }}</span>
                 </div>
                 <div class="flex flex-col" v-if="bankStatement.profile?.holders?.holder?.[0]?.nominee">
                   <span class="text-muted-foreground text-xs uppercase tracking-wider font-semibold">Nominee</span>
                   <span class="font-medium mt-0.5">{{ bankStatement.profile?.holders?.holder?.[0]?.nominee === 'REGISTERED' ? 'Registered' : 'Not Registered' }}</span>
                 </div>
               </div>
            </CardContent>
          </Card>
        </div>

        <Accordion type="single" collapsible class="w-full">
          <AccordionItem value="transactions" class="border rounded-lg bg-card text-card-foreground shadow-sm overflow-hidden" :disabled="!bankStatement.transactions?.transaction?.length">
            <AccordionTrigger class="group hover:no-underline px-4 py-4 data-[state=open]:border-b border-border">
              <span class="font-medium text-foreground text-lg text-left w-full pr-4">
                Transactions
                <span v-if="bankStatement.transactions?.xfina?.reordered" class="ml-2 text-xs font-normal text-muted-foreground" title="This bank prints some days out of order. Those days were put back into the order their running balances describe; no amounts were changed.">
                  reordered to match balances
                </span>
              </span>
              <template #icon>
                <div class="flex items-center gap-1.5 text-xs font-mono bg-primary/10 text-primary pl-2.5 pr-2 py-1.5 rounded shrink-0 ml-2">
                  <span>{{ bankStatement.transactions?.transaction?.length || 0 }} {{ bankStatement.transactions?.transaction?.length === 1 ? 'Txn' : 'Txns' }}</span>
                  <ChevronDown v-if="bankStatement.transactions?.transaction?.length" class="h-4 w-4 transition-transform duration-200 group-data-[state=open]:rotate-180" />
                </div>
              </template>
            </AccordionTrigger>
            <AccordionContent class="p-4">
              <div class="rounded-md border border-border overflow-x-auto">
              <Table>
                <TableHeader class="bg-muted/50">
                  <TableRow class="hover:bg-transparent">
                    <TableHead class="w-[150px] text-muted-foreground whitespace-nowrap">Date</TableHead>
                    <TableHead class="text-muted-foreground whitespace-nowrap">Description</TableHead>
                    <TableHead class="w-[140px] text-right text-muted-foreground whitespace-nowrap">Amount</TableHead>
                    <TableHead class="w-[140px] text-right text-muted-foreground whitespace-nowrap">Balance</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  <TableRow v-for="(txn, idx) in bankStatement.transactions?.transaction" :key="idx" class="hover:bg-muted/50 transition-colors">
                    <TableCell class="font-medium whitespace-nowrap">{{ formatDateTime(txn.xfina?.parsedDate || txn.transactionTimestamp, 'transactions.transaction.transactionTimestamp', bankStatement.xfina?.dateOnlyPaths) }}</TableCell>
                    <TableCell class="text-foreground text-sm">{{ txn.narration }}</TableCell>
                    <TableCell class="text-right font-mono whitespace-nowrap" :class="{'text-emerald-500': txn.type === 'CREDIT', 'text-foreground': txn.type !== 'CREDIT'}">
                        <span v-if="txn.type === 'CREDIT'">+</span>
                        {{ formatCurrency(txn.amount) }}
                    </TableCell>
                    <TableCell class="text-right font-mono font-medium whitespace-nowrap">{{ txn.currentBalance !== null && txn.currentBalance !== undefined ? formatCurrency(txn.currentBalance) : '-' }}</TableCell>
                  </TableRow>
                </TableBody>
              </Table>
            </div>
            </AccordionContent>
          </AccordionItem>
        </Accordion>
      </div>
      
      <!-- Equity Statement Results Table -->
      <div v-if="equityStatement" class="space-y-6">
        
        <!-- Standardized Header -->
        <StatementHeader
          :validationStatus="validationReport?.overall"
          :customerName="equityStatement.profile?.holders?.holder?.[0]?.name || 'Customer'"
          :address="equityStatement.profile?.holders?.holder?.[0]?.address || ''"
          :customerId="equityStatement.profile?.holders?.holder?.[0]?.xfina?.customerId || ''"
          :institutionName="equityStatement.xfina?.institutionName || 'Broker'"
          statementType="Equity / Brokerage"
          :accountNumber="equityStatement.maskedAccNumber || ''"
          :statementDetails="[
            ...(equityStatement.transactions?.startDate ? [{ label: 'From', value: formatDate(equityStatement.transactions.startDate) }] : []),
            ...(equityStatement.transactions?.endDate ? [{ label: 'To', value: formatDate(equityStatement.transactions.endDate) }] : []),
            ...(equityStatement.xfina?.generatedDate ? [{ label: 'Generated', value: formatDateTime(equityStatement.xfina.generatedDate, 'xfina.generatedDate', equityStatement.xfina?.dateOnlyPaths) }] : [])
          ]"
        />

        <div class="grid grid-cols-1 gap-4">
          <Card class="bg-card text-card-foreground shadow-sm">
            <CardHeader class="pb-2 border-b mb-3">
              <CardTitle class="text-sm text-muted-foreground font-semibold uppercase tracking-wider">Portfolio Summary</CardTitle>
            </CardHeader>
            <CardContent>
              <div class="grid grid-cols-2 md:grid-cols-4 gap-6">
                <div class="flex flex-col">
                  <span class="text-xs text-muted-foreground mb-1">Total Assets</span>
                  <span class="font-medium font-mono text-xl text-foreground">{{ equityStatement.summary?.investment?.holdings?.holding?.length || 0 }}</span>
                </div>
                <div class="flex flex-col">
                  <span class="text-xs text-muted-foreground mb-1">Total Invested</span>
                  <span class="font-medium font-mono text-xl">{{ formatCurrency(equityStatement.summary?.investmentValue) }}</span>
                </div>
                <div class="flex flex-col">
                  <span class="text-xs text-muted-foreground mb-1">Market Value</span>
                  <span class="font-medium font-mono text-xl text-primary">{{ formatCurrency(equityStatement.summary?.currentValue) }}</span>
                </div>
                <div class="flex flex-col">
                  <span class="text-xs text-muted-foreground mb-1">Unrealized P&L</span>
                  <span class="font-medium font-mono text-xl" 
                        :class="(equityStatement.summary?.currentValue || 0) >= (equityStatement.summary?.investmentValue || 0) ? 'text-emerald-500' : 'text-rose-500'">
                    {{ (equityStatement.summary?.currentValue || 0) >= (equityStatement.summary?.investmentValue || 0) ? '+ ' : '' }}{{ formatCurrency((equityStatement.summary?.currentValue || 0) - (equityStatement.summary?.investmentValue || 0)) }}
                  </span>
                </div>
              </div>
            </CardContent>
          </Card>
        </div>

        <Accordion type="multiple" class="w-full space-y-4">
           <AccordionItem 
             v-for="(holding, index) in equityStatement.summary?.investment?.holdings?.holding" 
             :key="index" 
             :value="`item-${index}`"
             class="border rounded-lg bg-card text-card-foreground shadow-sm overflow-hidden"
             :disabled="!getAssetTransactions(holding).length"
           >
               <div class="px-4 py-4 flex flex-col items-start w-full gap-3 border-b border-transparent transition-colors group-data-[state=open]/item:border-border group-data-[state=open]/item:border-b">
                 <div class="flex flex-col items-start w-full gap-3">
                   <!-- Top Row: Chevron, Name, Tags, Txn Pill -->
                   <div class="grid grid-cols-[auto_1fr_auto] items-start gap-4 w-full">
                     <span class="text-xs font-medium font-mono bg-muted/30 border border-primary/20 rounded px-2 py-0.5 text-primary shadow-sm shrink-0" v-if="holding.isin">{{ holding.isin }}</span>
                     
                     <div class="flex flex-wrap items-center gap-2 min-w-0">
                       <span class="font-medium text-foreground text-left text-base lg:text-lg leading-tight break-words">{{ holding.issuerName || holding.description }}</span>
                       <span class="text-xs font-medium font-mono bg-muted/30 border border-primary/20 rounded px-2 py-0.5 text-primary shadow-sm shrink-0">{{ holding.description || holding.issuerName }}</span>
                     </div>
                     
                     <AccordionTrigger class="py-1.5 flex-none font-mono text-xs font-normal hover:no-underline justify-end gap-1.5 bg-primary/10 text-primary hover:bg-primary/20 transition-colors pl-2.5 pr-2 rounded shrink-0 group w-auto" :disabled="!getAssetTransactions(holding).length">
                       <span>{{ getAssetTransactions(holding).length }} {{ getAssetTransactions(holding).length === 1 ? 'Txn' : 'Txns' }}</span>
                       <ChevronDown v-if="getAssetTransactions(holding).length" class="h-4 w-4 shrink-0 transition-transform duration-200 group-data-[state=open]:rotate-180" />
                       <template #icon><span class="hidden"></span></template>
                     </AccordionTrigger>
                   </div>
                   
                   <!-- 2-Column Blocks -->
                   <div class="flex flex-col lg:flex-row gap-3 w-full">
                     <div class="flex items-center justify-between text-xs bg-muted/20 border border-border rounded-md px-3.5 py-2.5 gap-3 flex-1 overflow-x-auto">
                       <div class="flex flex-col items-end shrink-0">
                         <span class="text-[10px] text-muted-foreground uppercase tracking-wider mb-0.5">Opening</span>
                         <span class="font-mono font-bold text-foreground text-sm text-right">{{ formatUnits(holding.xfina?.openingBalance || 0) }}</span>
                       </div>
                       <div class="w-px h-8 bg-border/60"></div>
                       <div class="flex flex-col items-end shrink-0">
                         <div class="flex items-center gap-1.5 mb-0.5">
                           <span v-if="holding.xfina?.periodBuyCount" class="text-xs text-muted-foreground bg-muted/50 px-1.5 py-0.5 rounded font-mono border border-border/50">{{ holding.xfina?.periodBuyCount }}</span>
                           <span class="text-[10px] text-muted-foreground uppercase tracking-wider">Buys</span>
                         </div>
                         <span class="font-mono font-bold text-foreground text-sm text-right">{{ formatUnits(holding.xfina?.periodBuyUnits || 0) }}</span>
                       </div>
                       <div class="w-px h-8 bg-border/60"></div>
                       <div class="flex flex-col items-end shrink-0">
                         <div class="flex items-center gap-1.5 mb-0.5">
                           <span v-if="holding.xfina?.periodSellCount" class="text-xs text-muted-foreground bg-muted/50 px-1.5 py-0.5 rounded font-mono border border-border/50">{{ holding.xfina?.periodSellCount }}</span>
                           <span class="text-[10px] text-muted-foreground uppercase tracking-wider">Sells</span>
                         </div>
                         <span class="font-mono font-bold text-foreground text-sm text-right">{{ formatUnits(holding.xfina?.periodSellUnits || 0) }}</span>
                       </div>
                       <div class="w-px h-8 bg-border/60"></div>
                       <div class="flex flex-col items-end shrink-0">
                         <span class="text-[10px] text-muted-foreground uppercase tracking-wider mb-0.5">Closing</span>
                         <span class="font-mono font-bold text-primary text-sm text-right">{{ formatUnits(holding.units) }}</span>
                       </div>
                       <div class="w-px h-8 bg-border/60"></div>
                       <div class="flex flex-col items-end shrink-0">
                         <span class="text-[10px] text-muted-foreground uppercase tracking-wider mb-0.5">NAV</span>
                         <span class="font-mono font-bold text-foreground text-sm text-right">{{ formatCurrency(holding.lastTradedPrice) }}</span>
                       </div>
                       <div class="w-px h-8 bg-border/60" v-if="equityStatement.xfina?.generatedDate"></div>
                       <div class="flex flex-col items-end shrink-0" v-if="equityStatement.xfina?.generatedDate">
                         <span class="text-[10px] text-muted-foreground uppercase tracking-wider mb-0.5">NAV Date</span>
                         <span class="font-mono font-medium text-foreground text-sm text-right">{{ formatDate(equityStatement.xfina?.generatedDate) }}</span>
                       </div>
                     </div>
                     
                     <div class="flex items-center text-right bg-muted/20 border border-border rounded-md px-3.5 py-2.5 shrink-0 gap-3">
                       <div class="flex flex-col items-end w-[130px] shrink-0">
                         <span class="text-[10px] text-muted-foreground uppercase tracking-wider mb-0.5">Total Invested</span>
                         <span class="text-sm font-medium font-mono text-foreground">{{ formatCurrency((holding.units || 0) * (holding.rate || 0)) }}</span>
                       </div>
                       <div class="w-px h-8 bg-border/60"></div>
                       <div class="flex flex-col items-end w-[130px] shrink-0">
                         <span class="text-[10px] text-muted-foreground uppercase tracking-wider mb-0.5">Market Value</span>
                         <span class="text-sm font-bold font-mono text-primary">{{ formatCurrency((holding.units || 0) * (holding.lastTradedPrice || 0)) }}</span>
                       </div>
                       <div class="w-px h-8 bg-border/60"></div>
                       <div class="flex flex-col items-end w-[130px] shrink-0">
                         <span class="text-[10px] text-muted-foreground uppercase tracking-wider mb-0.5">Unrealized P&L</span>
                         <span class="font-mono font-bold text-sm text-foreground">
                           {{ ((holding.units || 0) * (holding.lastTradedPrice || 0)) >= ((holding.units || 0) * (holding.rate || 0)) ? '+ ' : '' }}{{ formatCurrency(((holding.units || 0) * (holding.lastTradedPrice || 0)) - ((holding.units || 0) * (holding.rate || 0))) }}
                         </span>
                       </div>
                     </div>
                   </div>
                  </div>
                </div>
               <AccordionContent>
                 <div class="overflow-x-auto">
                   <Table>
                     <TableHeader class="bg-muted/50">
                       <TableRow class="hover:bg-transparent">
                         <TableHead class="w-[150px] text-muted-foreground whitespace-nowrap">Date</TableHead>
                         <TableHead class="w-[100px] text-muted-foreground whitespace-nowrap">Type</TableHead>
                         <TableHead class="text-muted-foreground whitespace-nowrap">Description</TableHead>
                         <TableHead class="w-[140px] text-right text-muted-foreground whitespace-nowrap">
                           <div class="flex items-center justify-end gap-1.5">
                             <span>Amount</span>
                             <TooltipProvider>
                               <Tooltip>
                                 <TooltipTrigger class="cursor-help">
                                   <HelpCircle class="h-3.5 w-3.5 text-muted-foreground" />
                                 </TooltipTrigger>
                                 <TooltipContent>
                                   <p class="max-w-[200px] text-xs font-normal whitespace-normal text-left">Invested (including the fee), if redeemed it is excluding the fee</p>
                                 </TooltipContent>
                               </Tooltip>
                             </TooltipProvider>
                           </div>
                         </TableHead>
                         <TableHead class="w-[120px] text-right text-muted-foreground whitespace-nowrap">Units</TableHead>
                         <TableHead class="w-[120px] text-right text-muted-foreground whitespace-nowrap">Price</TableHead>
                         <TableHead class="w-[120px] text-right text-muted-foreground whitespace-nowrap">Fees</TableHead>
                         <TableHead class="w-[140px] text-right text-muted-foreground whitespace-nowrap">Balance</TableHead>
                       </TableRow>
                     </TableHeader>
                     <TableBody>
                       <TableRow v-for="(txn, idx) in getAssetTransactions(holding)" :key="idx" class="hover:bg-muted/50 transition-colors">
                         <TableCell class="text-foreground whitespace-nowrap">{{ formatDateTime(txn.transactionDateTime, 'transactions.transaction.transactionDateTime', equityStatement.xfina?.dateOnlyPaths) }}</TableCell>
                         <TableCell class="text-foreground">
                            <span class="font-medium text-xs px-2 py-1 rounded" :class="{'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400': txn.type === 'BUY', 'bg-rose-100 text-rose-700 dark:bg-rose-900/30 dark:text-rose-400': txn.type === 'SELL'}">
                              {{ txn.type || '-' }}
                            </span>
                         </TableCell>
                         <TableCell class="text-foreground text-xs">-</TableCell>
                         <TableCell class="text-right font-mono text-foreground whitespace-nowrap">{{ formatCurrency(txn.tradeValue) }}</TableCell>
                         <TableCell class="text-right font-mono text-foreground whitespace-nowrap">{{ formatUnits(txn.units) }}</TableCell>
                         <TableCell class="text-right font-mono text-foreground whitespace-nowrap">{{ formatCurrency(txn.rate) }}</TableCell>
                         <TableCell class="text-right font-mono text-foreground whitespace-nowrap">{{ formatCurrency(txn.totalCharge) }}</TableCell>
                         <TableCell class="text-right font-mono text-foreground whitespace-nowrap">{{ formatUnits(txn._runningBalance) }}</TableCell>
                       </TableRow>
                     </TableBody>
                   </Table>
                 </div>
               </AccordionContent>
             </AccordionItem>
            </Accordion>
      </div>
      </template>
    </div>
  </div>
</template>
