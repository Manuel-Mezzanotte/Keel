import { useMemo, useState } from "react";

type Guest = {
  name: string;
  arrival: Date;
  departure: Date;
  adults: number;
  children: number;
};

type BookingCardProps = {
  unit: string;
  guests: Guest[];
  nightlyRate: number;
  onCheckIn?: (unit: string) => void;
};

const formatter = new Intl.NumberFormat("it-IT", {
  style: "currency",
  currency: "EUR",
});

function nightsBetween(arrival: Date, departure: Date): number {
  const milliseconds = departure.getTime() - arrival.getTime();
  return Math.max(0, Math.round(milliseconds / 86_400_000));
}

export function BookingCard({ unit, guests, nightlyRate, onCheckIn }: BookingCardProps) {
  const [expanded, setExpanded] = useState(false);

  const total = useMemo(() => {
    return guests.reduce((sum, guest) => {
      const nights = nightsBetween(guest.arrival, guest.departure);
      return sum + nights * nightlyRate * (guest.adults + guest.children * 0.5);
    }, 0);
  }, [guests, nightlyRate]);

  return (
    <article className="rounded-xl border p-4 shadow-sm">
      <header className="flex items-center justify-between">
        <h2 className="text-lg font-semibold">Unità {unit}</h2>
        <button type="button" onClick={() => setExpanded((value) => !value)}>
          {expanded ? "Chiudi" : "Dettagli"}
        </button>
      </header>

      {expanded && (
        <ul className="mt-3 space-y-1">
          {guests.map((guest) => (
            <li key={guest.name}>
              {guest.name}: {nightsBetween(guest.arrival, guest.departure)} notti
            </li>
          ))}
        </ul>
      )}

      <footer className="mt-4 flex items-center justify-between">
        <span>Totale: {formatter.format(total)}</span>
        <button type="button" onClick={() => onCheckIn?.(unit)} disabled={!onCheckIn}>
          Check-in
        </button>
      </footer>
    </article>
  );
}
