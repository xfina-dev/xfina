// Formatting shared by the personal and public views.

// Every timestamp the parsers emit is an instant in IST, dates included: a
// date with no time is midnight IST. So this reads them all in Asia/Kolkata —
// it used to read date-only fields in UTC, which only worked because those
// were stamped at midnight UTC and the ones beside them at midnight IST.
export const formatDate = (ts) => {
    if (ts === null || ts === undefined || ts === '') return '-';
    const d = new Date(Number(ts) * 1000);
    if (isNaN(d)) return ts;
    return new Intl.DateTimeFormat(undefined, {
        year: 'numeric',
        month: 'short',
        day: 'numeric',
        timeZone: 'Asia/Kolkata'
    }).format(d);
};

export const formatDateTime = (ts, path = null, dateOnlyPaths = []) => {
    if (ts === null || ts === undefined || ts === '') return '-';
    const d = new Date(Number(ts) * 1000);
    if (isNaN(d)) return ts;

    const forceDateOnly = path && dateOnlyPaths && dateOnlyPaths.includes(path);

    if (!forceDateOnly) {
        return new Intl.DateTimeFormat(undefined, {
            year: 'numeric',
            month: 'short',
            day: 'numeric',
            hour: '2-digit',
            minute: '2-digit',
            second: '2-digit',
            hour12: false,
            timeZone: 'Asia/Kolkata'
        }).format(d);
    } else {
        return new Intl.DateTimeFormat(undefined, {
            year: 'numeric',
            month: 'short',
            day: 'numeric',
            timeZone: 'Asia/Kolkata'
        }).format(d);
    }
};

/**
 * A calendar date written as "YYYY-MM-DD", as a series' coverage reports it.
 *
 * Read as a calendar date, not an instant: no timezone can move it to the day
 * before.
 */
export const formatIsoDate = (iso) => {
    if (!iso) return '-';
    const [y, m, d] = iso.split('-').map(Number);
    return new Intl.DateTimeFormat(undefined, {
        year: 'numeric',
        month: 'short',
        day: 'numeric',
        timeZone: 'UTC'
    }).format(new Date(Date.UTC(y, m - 1, d)));
};

/** "forex_travel_card_buy" as the sheet wrote it: "Forex Travel Card Buy". */
export const columnLabel = (key) => key
    .split('_')
    .map(word => (word.length <= 2 ? word.toUpperCase() : word[0].toUpperCase() + word.slice(1)))
    .join(' ');

/**
 * A rate as the sheet quotes it, with the unit it is quoted in.
 *
 * Some currencies are priced per hundred, and showing 60.36 for the yen
 * without saying so is off by two orders of magnitude.
 */
export const rateUnit = (currency) => (currency.unit > 1 ? `per ${currency.unit}` : '');
