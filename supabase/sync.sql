begin;

create table public.opon_records (
    owner uuid not null references auth.users(id) on delete cascade,
    key text not null,
    record jsonb not null,
    revision bigint not null check (revision > 0),
    primary key (owner, key)
);

alter table public.opon_records enable row level security;
revoke all on public.opon_records from anon, authenticated;
grant select, insert, update on public.opon_records to authenticated;

create policy "Only the workspace owner"
    on public.opon_records for all to authenticated
    using (owner = (select auth.uid()))
    with check (owner = (select auth.uid()));

create function public.opon_push(changes jsonb)
returns jsonb
language plpgsql
security invoker
set search_path = ''
as $$
declare
    account uuid := auth.uid();
    change jsonb;
    incoming jsonb;
    record_key text;
    saved public.opon_records%rowtype;
    replies jsonb := '[]'::jsonb;
begin
    if account is null then
        raise exception 'Sign in first';
    end if;
    perform pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended(account::text, 0));

    for change in select value from pg_catalog.jsonb_array_elements(changes)
    loop
        incoming := change->'record';
        record_key := incoming->>'key';
        if record_key is null or record_key !~ '^(note:[a-f0-9]{32}|folder:.+)$' then
            raise exception 'Invalid record key';
        end if;
        select * into saved
          from public.opon_records where owner = account and key = record_key;
        if not found and (change->>'revision')::bigint = 0 then
            insert into public.opon_records values (account, record_key, incoming, 1)
              returning * into saved;
        elsif found and saved.revision = (change->>'revision')::bigint and saved.record <> incoming then
            update public.opon_records set record = incoming, revision = saved.revision + 1
              where owner = account and key = record_key returning * into saved;
        end if;

        replies := replies || pg_catalog.jsonb_build_array(
            pg_catalog.jsonb_build_object('record', saved.record, 'revision', saved.revision)
        );
    end loop;
    return replies;
end;
$$;

create function public.opon_pull()
returns jsonb
language sql
security invoker
set search_path = ''
as $$
    select coalesce(
        pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object('record', record, 'revision', revision) order by key),
        '[]'::jsonb
    )
    from public.opon_records
    where owner = (select auth.uid());
$$;

revoke all on function public.opon_push(jsonb), public.opon_pull() from public, anon;
grant execute on function public.opon_push(jsonb), public.opon_pull() to authenticated;

commit;
