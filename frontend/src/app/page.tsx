import { SearchField } from "@/components/search-field";

export default function SearchPage() {
  return (
    <main className="flex min-h-dvh flex-col items-center px-4 pt-[22vh]">
      <h1 className="mb-8 text-6xl font-semibold tracking-tight text-brand select-none">ferret</h1>
      <SearchField />
    </main>
  );
}
