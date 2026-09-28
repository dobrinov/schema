SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: billing; Type: SCHEMA; Schema: -; Owner: -
--

CREATE SCHEMA billing;


--
-- Name: pgcrypto; Type: EXTENSION; Schema: -; Owner: -
--

CREATE EXTENSION IF NOT EXISTS pgcrypto WITH SCHEMA public;


--
-- Name: citext; Type: EXTENSION; Schema: -; Owner: -
--

CREATE EXTENSION IF NOT EXISTS citext WITH SCHEMA public;


--
-- Name: membership_role; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public.membership_role AS ENUM (
    'owner',
    'admin',
    'member',
    'guest'
);


--
-- Name: task_status; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public.task_status AS ENUM (
    'todo',
    'in_progress',
    'done'
);


--
-- Name: invoice_status; Type: TYPE; Schema: billing; Owner: -
--

CREATE TYPE billing.invoice_status AS ENUM (
    'draft',
    'open',
    'paid',
    'void'
);


--
-- Name: set_updated_at(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.set_updated_at() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  NEW.updated_at := now();
  RETURN NEW;
END;
$$;


SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: accounts; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.accounts (
    id bigint NOT NULL,
    name character varying NOT NULL,
    slug public.citext NOT NULL,
    plan_id bigint,
    settings jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp(6) without time zone NOT NULL,
    updated_at timestamp(6) without time zone NOT NULL
);


--
-- Name: TABLE accounts; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON TABLE public.accounts IS 'A tenant. Everything hangs off an account.';


--
-- Name: accounts_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.accounts_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: accounts_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.accounts_id_seq OWNED BY public.accounts.id;


--
-- Name: active_storage_attachments; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.active_storage_attachments (
    id bigint NOT NULL,
    name character varying NOT NULL,
    record_type character varying NOT NULL,
    record_id bigint NOT NULL,
    blob_id bigint NOT NULL,
    created_at timestamp(6) without time zone NOT NULL
);


CREATE SEQUENCE public.active_storage_attachments_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.active_storage_attachments_id_seq OWNED BY public.active_storage_attachments.id;


--
-- Name: active_storage_blobs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.active_storage_blobs (
    id bigint NOT NULL,
    key character varying NOT NULL,
    filename character varying NOT NULL,
    content_type character varying,
    metadata text,
    service_name character varying NOT NULL,
    byte_size bigint NOT NULL,
    checksum character varying,
    created_at timestamp(6) without time zone NOT NULL
);


CREATE SEQUENCE public.active_storage_blobs_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.active_storage_blobs_id_seq OWNED BY public.active_storage_blobs.id;


--
-- Name: active_storage_variant_records; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.active_storage_variant_records (
    id bigint NOT NULL,
    blob_id bigint NOT NULL,
    variation_digest character varying NOT NULL
);


CREATE SEQUENCE public.active_storage_variant_records_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.active_storage_variant_records_id_seq OWNED BY public.active_storage_variant_records.id;


--
-- Name: api_keys; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.api_keys (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    account_id bigint NOT NULL,
    created_by_id bigint,
    token_digest character varying NOT NULL,
    last_used_at timestamp(6) without time zone,
    revoked_at timestamp(6) without time zone,
    created_at timestamp(6) without time zone NOT NULL
);


--
-- Name: ar_internal_metadata; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.ar_internal_metadata (
    key character varying NOT NULL,
    value character varying,
    created_at timestamp(6) without time zone NOT NULL,
    updated_at timestamp(6) without time zone NOT NULL
);


--
-- Name: audit_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events (
    id bigint NOT NULL,
    account_id bigint NOT NULL,
    actor_id bigint,
    action character varying NOT NULL,
    subject_type character varying,
    subject_id bigint,
    payload jsonb DEFAULT '{}'::jsonb NOT NULL,
    occurred_at timestamp(6) without time zone NOT NULL
)
PARTITION BY RANGE (occurred_at);


--
-- Name: audit_events_2025; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events_2025 (
    id bigint NOT NULL,
    account_id bigint NOT NULL,
    actor_id bigint,
    action character varying NOT NULL,
    subject_type character varying,
    subject_id bigint,
    payload jsonb DEFAULT '{}'::jsonb NOT NULL,
    occurred_at timestamp(6) without time zone NOT NULL
);


--
-- Name: audit_events_2026; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events_2026 (
    id bigint NOT NULL,
    account_id bigint NOT NULL,
    actor_id bigint,
    action character varying NOT NULL,
    subject_type character varying,
    subject_id bigint,
    payload jsonb DEFAULT '{}'::jsonb NOT NULL,
    occurred_at timestamp(6) without time zone NOT NULL
);


--
-- Name: comments; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.comments (
    id bigint NOT NULL,
    task_id bigint NOT NULL,
    author_id bigint NOT NULL,
    parent_id bigint,
    body text NOT NULL,
    edited_at timestamp(6) without time zone,
    created_at timestamp(6) without time zone NOT NULL,
    updated_at timestamp(6) without time zone NOT NULL
);


CREATE SEQUENCE public.comments_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.comments_id_seq OWNED BY public.comments.id;


--
-- Name: invoice_line_items; Type: TABLE; Schema: billing; Owner: -
--

CREATE TABLE billing.invoice_line_items (
    id bigint NOT NULL,
    invoice_id bigint NOT NULL,
    description character varying NOT NULL,
    quantity integer DEFAULT 1 NOT NULL,
    unit_amount_cents integer NOT NULL,
    created_at timestamp(6) without time zone NOT NULL,
    CONSTRAINT positive_quantity CHECK ((quantity > 0))
);


CREATE SEQUENCE billing.invoice_line_items_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE billing.invoice_line_items_id_seq OWNED BY billing.invoice_line_items.id;


--
-- Name: invoices; Type: TABLE; Schema: billing; Owner: -
--

CREATE TABLE billing.invoices (
    id bigint NOT NULL,
    account_id bigint NOT NULL,
    subscription_id bigint,
    number character varying NOT NULL,
    status billing.invoice_status DEFAULT 'draft'::billing.invoice_status NOT NULL,
    total_cents integer DEFAULT 0 NOT NULL,
    currency character(3) DEFAULT 'USD'::bpchar NOT NULL,
    due_on date,
    paid_at timestamp(6) without time zone,
    created_at timestamp(6) without time zone NOT NULL,
    updated_at timestamp(6) without time zone NOT NULL
);


CREATE SEQUENCE billing.invoices_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE billing.invoices_id_seq OWNED BY billing.invoices.id;


--
-- Name: memberships; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.memberships (
    id bigint NOT NULL,
    account_id bigint NOT NULL,
    user_id bigint NOT NULL,
    role public.membership_role DEFAULT 'member'::public.membership_role NOT NULL,
    invited_by_id bigint,
    created_at timestamp(6) without time zone NOT NULL,
    updated_at timestamp(6) without time zone NOT NULL
);


CREATE SEQUENCE public.memberships_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.memberships_id_seq OWNED BY public.memberships.id;


--
-- Name: notifications; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.notifications (
    id bigint NOT NULL,
    recipient_id bigint NOT NULL,
    notifiable_type character varying NOT NULL,
    notifiable_id bigint NOT NULL,
    kind character varying NOT NULL,
    read_at timestamp(6) without time zone,
    created_at timestamp(6) without time zone NOT NULL
);


CREATE SEQUENCE public.notifications_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.notifications_id_seq OWNED BY public.notifications.id;


--
-- Name: payments; Type: TABLE; Schema: billing; Owner: -
--

CREATE TABLE billing.payments (
    id bigint NOT NULL,
    invoice_id bigint NOT NULL,
    amount_cents integer NOT NULL,
    provider character varying NOT NULL,
    provider_reference character varying,
    succeeded_at timestamp(6) without time zone,
    created_at timestamp(6) without time zone NOT NULL
);


CREATE SEQUENCE billing.payments_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE billing.payments_id_seq OWNED BY billing.payments.id;


--
-- Name: plans; Type: TABLE; Schema: billing; Owner: -
--

CREATE TABLE billing.plans (
    id bigint NOT NULL,
    code character varying NOT NULL,
    name character varying NOT NULL,
    monthly_price_cents integer NOT NULL,
    seats integer,
    active boolean DEFAULT true NOT NULL
);


CREATE SEQUENCE billing.plans_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE billing.plans_id_seq OWNED BY billing.plans.id;


--
-- Name: projects; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.projects (
    id bigint NOT NULL,
    account_id bigint NOT NULL,
    owner_id bigint,
    name character varying NOT NULL,
    description text,
    archived_at timestamp(6) without time zone,
    created_at timestamp(6) without time zone NOT NULL,
    updated_at timestamp(6) without time zone NOT NULL
);


CREATE SEQUENCE public.projects_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.projects_id_seq OWNED BY public.projects.id;


--
-- Name: schema_migrations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.schema_migrations (
    version character varying NOT NULL
);


--
-- Name: subscriptions; Type: TABLE; Schema: billing; Owner: -
--

CREATE TABLE billing.subscriptions (
    id bigint NOT NULL,
    account_id bigint NOT NULL,
    plan_id bigint NOT NULL,
    starts_on date NOT NULL,
    ends_on date,
    canceled_at timestamp(6) without time zone,
    created_at timestamp(6) without time zone NOT NULL,
    updated_at timestamp(6) without time zone NOT NULL
);


CREATE SEQUENCE billing.subscriptions_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE billing.subscriptions_id_seq OWNED BY billing.subscriptions.id;


--
-- Name: taggings; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.taggings (
    id bigint NOT NULL,
    tag_id bigint NOT NULL,
    task_id bigint NOT NULL,
    created_at timestamp(6) without time zone NOT NULL
);


CREATE SEQUENCE public.taggings_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.taggings_id_seq OWNED BY public.taggings.id;


--
-- Name: tags; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.tags (
    id bigint NOT NULL,
    account_id bigint NOT NULL,
    name character varying NOT NULL,
    color character varying(7)
);


CREATE SEQUENCE public.tags_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.tags_id_seq OWNED BY public.tags.id;


--
-- Name: tasks; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.tasks (
    id bigint NOT NULL,
    project_id bigint NOT NULL,
    assignee_id bigint,
    reporter_id bigint NOT NULL,
    parent_task_id bigint,
    title character varying NOT NULL,
    body text,
    status public.task_status DEFAULT 'todo'::public.task_status NOT NULL,
    priority integer DEFAULT 0 NOT NULL,
    due_on date,
    completed_at timestamp(6) without time zone,
    position integer,
    created_at timestamp(6) without time zone NOT NULL,
    updated_at timestamp(6) without time zone NOT NULL
);


COMMENT ON COLUMN public.tasks.position IS 'Manual sort order within a project.';


CREATE SEQUENCE public.tasks_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.tasks_id_seq OWNED BY public.tasks.id;


--
-- Name: users; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.users (
    id bigint NOT NULL,
    email public.citext NOT NULL,
    name character varying,
    encrypted_password character varying DEFAULT ''::character varying NOT NULL,
    reset_password_token character varying,
    reset_password_sent_at timestamp(6) without time zone,
    last_sign_in_at timestamp(6) without time zone,
    time_zone character varying DEFAULT 'UTC'::character varying NOT NULL,
    created_at timestamp(6) without time zone NOT NULL,
    updated_at timestamp(6) without time zone NOT NULL
);


CREATE SEQUENCE public.users_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.users_id_seq OWNED BY public.users.id;


--
-- Name: open_tasks; Type: VIEW; Schema: public; Owner: -
--

CREATE VIEW public.open_tasks AS
 SELECT tasks.id,
    tasks.project_id,
    tasks.assignee_id,
    tasks.title,
    projects.account_id
   FROM (public.tasks
     JOIN public.projects ON ((projects.id = tasks.project_id)))
  WHERE (tasks.status <> 'done'::public.task_status);


--
-- Name: account_revenue; Type: MATERIALIZED VIEW; Schema: billing; Owner: -
--

CREATE MATERIALIZED VIEW billing.account_revenue AS
 SELECT invoices.account_id,
    sum(invoices.total_cents) AS total_cents
   FROM billing.invoices
  WHERE (invoices.status = 'paid'::billing.invoice_status)
  GROUP BY invoices.account_id
  WITH NO DATA;


ALTER TABLE ONLY public.audit_events ATTACH PARTITION public.audit_events_2025 FOR VALUES FROM ('2025-01-01 00:00:00') TO ('2026-01-01 00:00:00');

ALTER TABLE ONLY public.audit_events ATTACH PARTITION public.audit_events_2026 FOR VALUES FROM ('2026-01-01 00:00:00') TO ('2027-01-01 00:00:00');

ALTER TABLE ONLY billing.invoice_line_items ALTER COLUMN id SET DEFAULT nextval('billing.invoice_line_items_id_seq'::regclass);

ALTER TABLE ONLY billing.invoices ALTER COLUMN id SET DEFAULT nextval('billing.invoices_id_seq'::regclass);

ALTER TABLE ONLY billing.payments ALTER COLUMN id SET DEFAULT nextval('billing.payments_id_seq'::regclass);

ALTER TABLE ONLY billing.plans ALTER COLUMN id SET DEFAULT nextval('billing.plans_id_seq'::regclass);

ALTER TABLE ONLY billing.subscriptions ALTER COLUMN id SET DEFAULT nextval('billing.subscriptions_id_seq'::regclass);

ALTER TABLE ONLY public.accounts ALTER COLUMN id SET DEFAULT nextval('public.accounts_id_seq'::regclass);

ALTER TABLE ONLY public.active_storage_attachments ALTER COLUMN id SET DEFAULT nextval('public.active_storage_attachments_id_seq'::regclass);

ALTER TABLE ONLY public.active_storage_blobs ALTER COLUMN id SET DEFAULT nextval('public.active_storage_blobs_id_seq'::regclass);

ALTER TABLE ONLY public.active_storage_variant_records ALTER COLUMN id SET DEFAULT nextval('public.active_storage_variant_records_id_seq'::regclass);

ALTER TABLE ONLY public.comments ALTER COLUMN id SET DEFAULT nextval('public.comments_id_seq'::regclass);

ALTER TABLE ONLY public.memberships ALTER COLUMN id SET DEFAULT nextval('public.memberships_id_seq'::regclass);

ALTER TABLE ONLY public.notifications ALTER COLUMN id SET DEFAULT nextval('public.notifications_id_seq'::regclass);

ALTER TABLE ONLY public.projects ALTER COLUMN id SET DEFAULT nextval('public.projects_id_seq'::regclass);

ALTER TABLE ONLY public.taggings ALTER COLUMN id SET DEFAULT nextval('public.taggings_id_seq'::regclass);

ALTER TABLE ONLY public.tags ALTER COLUMN id SET DEFAULT nextval('public.tags_id_seq'::regclass);

ALTER TABLE ONLY public.tasks ALTER COLUMN id SET DEFAULT nextval('public.tasks_id_seq'::regclass);

ALTER TABLE ONLY public.users ALTER COLUMN id SET DEFAULT nextval('public.users_id_seq'::regclass);

ALTER TABLE ONLY billing.invoice_line_items
    ADD CONSTRAINT invoice_line_items_pkey PRIMARY KEY (id);

ALTER TABLE ONLY billing.invoices
    ADD CONSTRAINT invoices_pkey PRIMARY KEY (id);

ALTER TABLE ONLY billing.payments
    ADD CONSTRAINT payments_pkey PRIMARY KEY (id);

ALTER TABLE ONLY billing.plans
    ADD CONSTRAINT plans_pkey PRIMARY KEY (id);

ALTER TABLE ONLY billing.subscriptions
    ADD CONSTRAINT subscriptions_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.accounts
    ADD CONSTRAINT accounts_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.active_storage_attachments
    ADD CONSTRAINT active_storage_attachments_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.active_storage_blobs
    ADD CONSTRAINT active_storage_blobs_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.active_storage_variant_records
    ADD CONSTRAINT active_storage_variant_records_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.api_keys
    ADD CONSTRAINT api_keys_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.ar_internal_metadata
    ADD CONSTRAINT ar_internal_metadata_pkey PRIMARY KEY (key);

ALTER TABLE ONLY public.audit_events
    ADD CONSTRAINT audit_events_pkey PRIMARY KEY (id, occurred_at);

ALTER TABLE ONLY public.audit_events_2025
    ADD CONSTRAINT audit_events_2025_pkey PRIMARY KEY (id, occurred_at);

ALTER TABLE ONLY public.audit_events_2026
    ADD CONSTRAINT audit_events_2026_pkey PRIMARY KEY (id, occurred_at);

ALTER TABLE ONLY public.comments
    ADD CONSTRAINT comments_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.memberships
    ADD CONSTRAINT memberships_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.notifications
    ADD CONSTRAINT notifications_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.projects
    ADD CONSTRAINT projects_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.schema_migrations
    ADD CONSTRAINT schema_migrations_pkey PRIMARY KEY (version);

ALTER TABLE ONLY public.taggings
    ADD CONSTRAINT taggings_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.tags
    ADD CONSTRAINT tags_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.tasks
    ADD CONSTRAINT tasks_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.users
    ADD CONSTRAINT users_pkey PRIMARY KEY (id);

CREATE UNIQUE INDEX index_accounts_on_slug ON public.accounts USING btree (slug);

CREATE INDEX index_active_storage_attachments_on_blob_id ON public.active_storage_attachments USING btree (blob_id);

CREATE UNIQUE INDEX index_active_storage_attachments_uniqueness ON public.active_storage_attachments USING btree (record_type, record_id, name, blob_id);

CREATE UNIQUE INDEX index_active_storage_blobs_on_key ON public.active_storage_blobs USING btree (key);

CREATE UNIQUE INDEX index_active_storage_variant_records_uniqueness ON public.active_storage_variant_records USING btree (blob_id, variation_digest);

CREATE INDEX index_api_keys_on_account_id ON public.api_keys USING btree (account_id);

CREATE UNIQUE INDEX index_api_keys_on_token_digest ON public.api_keys USING btree (token_digest);

CREATE INDEX index_audit_events_on_account_id_and_occurred_at ON ONLY public.audit_events USING btree (account_id, occurred_at);

CREATE INDEX audit_events_2025_account_id_occurred_at_idx ON public.audit_events_2025 USING btree (account_id, occurred_at);

CREATE INDEX audit_events_2026_account_id_occurred_at_idx ON public.audit_events_2026 USING btree (account_id, occurred_at);

CREATE INDEX index_comments_on_author_id ON public.comments USING btree (author_id);

CREATE INDEX index_comments_on_parent_id ON public.comments USING btree (parent_id);

CREATE INDEX index_comments_on_task_id ON public.comments USING btree (task_id);

CREATE INDEX index_invoice_line_items_on_invoice_id ON billing.invoice_line_items USING btree (invoice_id);

CREATE INDEX index_invoices_on_account_id ON billing.invoices USING btree (account_id);

CREATE UNIQUE INDEX index_invoices_on_number ON billing.invoices USING btree (number);

CREATE INDEX index_invoices_on_subscription_id ON billing.invoices USING btree (subscription_id);

CREATE UNIQUE INDEX index_memberships_on_account_id_and_user_id ON public.memberships USING btree (account_id, user_id);

CREATE INDEX index_memberships_on_user_id ON public.memberships USING btree (user_id);

CREATE INDEX index_notifications_on_notifiable ON public.notifications USING btree (notifiable_type, notifiable_id);

CREATE INDEX index_notifications_on_recipient_id_unread ON public.notifications USING btree (recipient_id) WHERE (read_at IS NULL);

CREATE INDEX index_payments_on_invoice_id ON billing.payments USING btree (invoice_id);

CREATE UNIQUE INDEX index_plans_on_code ON billing.plans USING btree (code);

CREATE INDEX index_projects_on_account_id ON public.projects USING btree (account_id);

CREATE INDEX index_subscriptions_on_account_id ON billing.subscriptions USING btree (account_id);

CREATE UNIQUE INDEX index_taggings_on_tag_id_and_task_id ON public.taggings USING btree (tag_id, task_id);

CREATE UNIQUE INDEX index_tags_on_account_id_and_name ON public.tags USING btree (account_id, lower((name)::text));

CREATE INDEX index_tasks_on_assignee_id ON public.tasks USING btree (assignee_id);

CREATE INDEX index_tasks_on_project_id_and_position ON public.tasks USING btree (project_id, "position");

CREATE UNIQUE INDEX index_users_on_email ON public.users USING btree (email);

CREATE UNIQUE INDEX index_users_on_reset_password_token ON public.users USING btree (reset_password_token);

CREATE UNIQUE INDEX index_account_revenue_on_account_id ON billing.account_revenue USING btree (account_id);

ALTER INDEX public.index_audit_events_on_account_id_and_occurred_at ATTACH PARTITION public.audit_events_2025_account_id_occurred_at_idx;

ALTER INDEX public.index_audit_events_on_account_id_and_occurred_at ATTACH PARTITION public.audit_events_2026_account_id_occurred_at_idx;

CREATE TRIGGER set_updated_at_on_tasks BEFORE UPDATE ON public.tasks FOR EACH ROW EXECUTE FUNCTION public.set_updated_at();

CREATE TRIGGER set_updated_at_on_projects BEFORE UPDATE ON public.projects FOR EACH ROW EXECUTE FUNCTION public.set_updated_at();

ALTER TABLE ONLY public.memberships
    ADD CONSTRAINT fk_rails_0a1b2c3d4e FOREIGN KEY (account_id) REFERENCES public.accounts(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.memberships
    ADD CONSTRAINT fk_rails_1b2c3d4e5f FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.memberships
    ADD CONSTRAINT fk_rails_2c3d4e5f6a FOREIGN KEY (invited_by_id) REFERENCES public.users(id);

ALTER TABLE ONLY public.accounts
    ADD CONSTRAINT fk_rails_3d4e5f6a7b FOREIGN KEY (plan_id) REFERENCES billing.plans(id);

ALTER TABLE ONLY public.projects
    ADD CONSTRAINT fk_rails_4e5f6a7b8c FOREIGN KEY (account_id) REFERENCES public.accounts(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.projects
    ADD CONSTRAINT fk_rails_5f6a7b8c9d FOREIGN KEY (owner_id) REFERENCES public.users(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.tasks
    ADD CONSTRAINT fk_rails_6a7b8c9d0e FOREIGN KEY (project_id) REFERENCES public.projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.tasks
    ADD CONSTRAINT fk_rails_7b8c9d0e1f FOREIGN KEY (assignee_id) REFERENCES public.users(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.tasks
    ADD CONSTRAINT fk_rails_8c9d0e1f2a FOREIGN KEY (reporter_id) REFERENCES public.users(id);

ALTER TABLE ONLY public.tasks
    ADD CONSTRAINT fk_rails_9d0e1f2a3b FOREIGN KEY (parent_task_id) REFERENCES public.tasks(id);

ALTER TABLE ONLY public.comments
    ADD CONSTRAINT fk_rails_0e1f2a3b4c FOREIGN KEY (task_id) REFERENCES public.tasks(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.comments
    ADD CONSTRAINT fk_rails_1f2a3b4c5d FOREIGN KEY (author_id) REFERENCES public.users(id);

ALTER TABLE ONLY public.comments
    ADD CONSTRAINT fk_rails_2a3b4c5d6e FOREIGN KEY (parent_id) REFERENCES public.comments(id);

ALTER TABLE ONLY public.tags
    ADD CONSTRAINT fk_rails_3b4c5d6e7f FOREIGN KEY (account_id) REFERENCES public.accounts(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.taggings
    ADD CONSTRAINT fk_rails_4c5d6e7f8a FOREIGN KEY (tag_id) REFERENCES public.tags(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.taggings
    ADD CONSTRAINT fk_rails_5d6e7f8a9b FOREIGN KEY (task_id) REFERENCES public.tasks(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.notifications
    ADD CONSTRAINT fk_rails_6e7f8a9b0c FOREIGN KEY (recipient_id) REFERENCES public.users(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.api_keys
    ADD CONSTRAINT fk_rails_7f8a9b0c1d FOREIGN KEY (account_id) REFERENCES public.accounts(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.api_keys
    ADD CONSTRAINT fk_rails_8a9b0c1d2e FOREIGN KEY (created_by_id) REFERENCES public.users(id) ON DELETE SET NULL;

ALTER TABLE public.audit_events
    ADD CONSTRAINT fk_rails_9b0c1d2e3f FOREIGN KEY (account_id) REFERENCES public.accounts(id);

ALTER TABLE ONLY public.active_storage_attachments
    ADD CONSTRAINT fk_rails_c3b3935057 FOREIGN KEY (blob_id) REFERENCES public.active_storage_blobs(id);

ALTER TABLE ONLY public.active_storage_variant_records
    ADD CONSTRAINT fk_rails_993965df05 FOREIGN KEY (blob_id) REFERENCES public.active_storage_blobs(id);

ALTER TABLE ONLY billing.subscriptions
    ADD CONSTRAINT fk_rails_a0b1c2d3e4 FOREIGN KEY (account_id) REFERENCES public.accounts(id) ON DELETE CASCADE;

ALTER TABLE ONLY billing.subscriptions
    ADD CONSTRAINT fk_rails_b1c2d3e4f5 FOREIGN KEY (plan_id) REFERENCES billing.plans(id);

ALTER TABLE ONLY billing.invoices
    ADD CONSTRAINT fk_rails_c2d3e4f5a6 FOREIGN KEY (account_id) REFERENCES public.accounts(id);

ALTER TABLE ONLY billing.invoices
    ADD CONSTRAINT fk_rails_d3e4f5a6b7 FOREIGN KEY (subscription_id) REFERENCES billing.subscriptions(id);

ALTER TABLE ONLY billing.invoice_line_items
    ADD CONSTRAINT fk_rails_e4f5a6b7c8 FOREIGN KEY (invoice_id) REFERENCES billing.invoices(id) ON DELETE CASCADE;

ALTER TABLE ONLY billing.payments
    ADD CONSTRAINT fk_rails_f5a6b7c8d9 FOREIGN KEY (invoice_id) REFERENCES billing.invoices(id);

--
-- PostgreSQL database dump complete
--

SET search_path TO "$user", public;

INSERT INTO "schema_migrations" (version) VALUES
('20250101000000'),
('20250201000000'),
('20260301000000');

