<script lang="ts">
  import { onMount } from 'svelte';
  import { session, waitForSession } from '$lib/session.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import { loadUsageMetrics } from './data';
  import {
    metricErrorLabel,
    metricNumber,
    metricTime,
    successRate,
    trendPath,
    type MetricsPeriod,
    type MetricsSnapshot
  } from './model';

  type MetricsView =
    | { type: 'loading' }
    | { type: 'ready'; data: MetricsSnapshot }
    | { type: 'error'; message: string };
  let view = $state<MetricsView>({ type: 'loading' });
  let period = $state<MetricsPeriod>('24h');
  let controller: AbortController | undefined;
  let requestId = 0;
  const periods: { value: MetricsPeriod; label: string }[] = [
    { value: '24h', label: '24 horas' },
    { value: '7d', label: '7 dias' },
    { value: '30d', label: '30 dias' }
  ];

  onMount(() => {
    void load();
    return () => {
      controller?.abort();
    };
  });
  async function load(selectedPeriod: MetricsPeriod = period): Promise<void> {
    controller?.abort();
    const active = new AbortController();
    controller = active;
    const request = ++requestId;
    period = selectedPeriod;
    view = { type: 'loading' };
    const timeout = setTimeout(() => {
      active.abort();
    }, 35_000);
    try {
      await waitForSession();
      if (!session.currentPrincipal) {
        return;
      }
      const data = await loadUsageMetrics(
        selectedPeriod,
        session.currentPrincipal.platform_role === 'admin' ? 'workspace' : 'personal',
        active.signal
      );
      if (request === requestId) {
        view = { type: 'ready', data };
      }
    } catch (cause) {
      if (request === requestId) {
        view = {
          type: 'error',
          message: active.signal.aborted
            ? 'O carregamento demorou mais que o esperado. Tente novamente.'
            : cause instanceof Error
              ? cause.message
              : 'Não foi possível carregar as métricas.'
        };
      }
    } finally {
      clearTimeout(timeout);
    }
  }
</script>

<section class="usage-metrics" aria-labelledby="usage-title">
  <div class="metrics-heading">
    <div>
      <h2 id="usage-title">
        {session.currentPrincipal?.platform_role === 'admin'
          ? 'Atividade do workspace'
          : 'Sua atividade no MCP'}
      </h2>
      <p class="section-description">
        Consultas registradas na auditoria, sem limite de 100 eventos.
      </p>
    </div>
    <div class="metrics-controls">
      <div class="period-picker" role="group" aria-label="Período das métricas">
        {#each periods as choice}<button
            aria-pressed={period === choice.value}
            class:chosen={period === choice.value}
            disabled={view.type === 'loading'}
            onclick={() => {
              void load(choice.value);
            }}>{choice.label}</button
          >{/each}
      </div>
      <button
        class="icon-button"
        aria-label="Atualizar métricas"
        title="Atualizar métricas"
        disabled={view.type === 'loading'}
        onclick={() => {
          void load();
        }}><Icon name="refresh" size={18} /></button
      >
    </div>
  </div>
  {#if view.type === 'loading'}<div class="loading-panel" role="status">
      Calculando métricas do período…
    </div>
  {:else if view.type === 'error'}<div class="notice notice-error" role="alert">
      <p>{view.message}</p>
      <button
        class="button button-secondary"
        onclick={() => {
          void load();
        }}>Tentar carregar métricas novamente</button
      >
    </div>
  {:else}
    {@const data = view.data}
    {@const summary = data.summary}
    {@const rate = successRate(summary)}
    {@const maxVolume = Math.max(0, ...data.series.map((bucket) => bucket.total_queries))}
    <div class="metrics-context">
      <span>{metricTime(data.from, true)} a {metricTime(data.to, true)} UTC</span><span
        >Atualizado às {new Date(data.to).toLocaleTimeString('pt-BR', {
          timeZone: 'UTC',
          hour: '2-digit',
          minute: '2-digit'
        })} UTC · {data.scope === 'workspace' ? 'Todo o workspace' : 'Somente sua identidade'}</span
      >
    </div>
    <div class="usage-kpis" aria-label="Indicadores do período">
      <article class="usage-kpi kpi-volume">
        <span>Consultas registradas</span><strong>{metricNumber(summary.total_queries)}</strong>
        <p>
          {metricNumber(summary.read_queries)} leituras · {metricNumber(summary.write_queries)} escritas
        </p>
      </article>
      <article class="usage-kpi">
        <span>Taxa de sucesso</span><strong
          >{rate === undefined ? '—' : `${metricNumber(rate)}%`}</strong
        >
        <p>{metricNumber(summary.successful_queries)} concluídas</p>
      </article>
      <article class="usage-kpi">
        <span>Latência p95</span><strong
          >{summary.p95_duration_ms === undefined
            ? '—'
            : metricNumber(
                summary.p95_duration_ms
              )}{#if summary.p95_duration_ms !== undefined}<small>ms</small>{/if}</strong
        >
        <p>
          {summary.avg_duration_ms === undefined
            ? 'Sem medições de sucesso'
            : `Média: ${metricNumber(summary.avg_duration_ms)} ms`}
        </p>
      </article>
      <article class="usage-kpi">
        <span>Falhas</span><strong class:metric-negative={summary.failed_queries > 0}
          >{metricNumber(summary.failed_queries)}</strong
        >
        <p>Consultas não concluídas</p>
      </article>
      <article class="usage-kpi">
        <span>Acessos negados</span><strong class:metric-warning={summary.denied_queries > 0}
          >{metricNumber(summary.denied_queries)}</strong
        >
        <p>Bloqueados pela permissão</p>
      </article>
      <article class="usage-kpi">
        <span>{data.scope === 'workspace' ? 'Pessoas com atividade' : 'Bancos utilizados'}</span
        ><strong
          >{metricNumber(
            data.scope === 'workspace' ? summary.active_users : summary.used_databases
          )}</strong
        >
        <p>
          {data.scope === 'workspace'
            ? `${metricNumber(summary.used_databases)} bancos no período`
            : 'IDs de bancos nos seus registros'}
        </p>
      </article>
    </div>
    <div class="metrics-detail-grid">
      <article class="panel trend-panel">
        <div class="panel-heading">
          <div>
            <h3>Volume de consultas</h3>
            <p class="section-description">
              {data.bucket_seconds === 3600 ? 'Intervalos de 1 hora' : 'Intervalos de 24 horas'} · UTC
            </p>
          </div>
          <span class="badge">Pico: {metricNumber(maxVolume)}</span>
        </div>
        {#if summary.total_queries === 0}<div class="empty-state compact-empty">
            <Icon name="audit" size={30} />
            <h3>Nenhuma consulta neste período</h3>
            <p>Faça uma consulta pelo cliente MCP ou selecione um período maior.</p>
            <a class="text-link" href="/connect"
              >Configurar meu cliente <Icon name="arrow" size={16} /></a
            >
          </div>
        {:else}<div class="volume-chart">
            <div class="chart-scale" aria-hidden="true">
              <span>{metricNumber(maxVolume)}</span><span>{metricNumber(maxVolume / 2)}</span><span
                >0</span
              >
            </div>
            <svg
              viewBox="0 0 720 180"
              preserveAspectRatio="none"
              role="img"
              aria-label={`Volume de ${summary.total_queries} consultas no período, em ${data.series.length} intervalos. Pico de ${maxVolume}. Valores exatos na tabela abaixo.`}
              ><path d="M0,20H720 M0,95H720 M0,170H720" class="chart-grid" /><path
                d={trendPath(data.series)}
                class="chart-line"
                vector-effect="non-scaling-stroke"
              /></svg
            >
          </div>
          <div class="chart-ticks" aria-hidden="true">
            <span>{metricTime(data.from, data.bucket_seconds === 3600)}</span><span
              >{metricTime(
                data.series[Math.floor(data.series.length / 2)].start,
                data.bucket_seconds === 3600
              )}</span
            ><span
              >{metricTime(
                data.series[data.series.length - 1].start,
                data.bucket_seconds === 3600
              )}</span
            >
          </div>{/if}
        <details class="metric-data-table">
          <summary>Ver valores por intervalo</summary>
          <div class="table-scroll">
            <table>
              <caption class="sr-only">Consultas registradas por intervalo, em UTC</caption><thead
                ><tr
                  ><th scope="col">Início (UTC)</th><th scope="col">Total</th><th scope="col"
                    >Sucesso</th
                  ><th scope="col">Falha</th><th scope="col">Negado</th></tr
                ></thead
              ><tbody
                >{#each data.series as bucket}<tr
                    ><td>{metricTime(bucket.start, true)}</td><td
                      >{metricNumber(bucket.total_queries)}</td
                    ><td>{metricNumber(bucket.successful_queries)}</td><td
                      >{metricNumber(bucket.failed_queries)}</td
                    ><td>{metricNumber(bucket.denied_queries)}</td></tr
                  >{/each}</tbody
              >
            </table>
          </div>
        </details>
      </article>
      <article class="panel database-ranking">
        <div class="panel-heading">
          <div>
            <h3>Bancos mais consultados</h3>
            <p class="section-description">
              Até 5 bancos · {data.scope === 'personal' ? 'com acesso atual' : 'no período'}
            </p>
          </div>
        </div>
        {#if data.top_databases.length === 0}<p class="ranking-empty">
            {data.scope === 'personal' && summary.total_queries > 0
              ? 'Nenhum banco com permissão atual neste ranking. Seu histórico agregado continua visível acima.'
              : 'O ranking aparece após a primeira consulta registrada.'}
          </p>{:else}<ol class="ranking-list">
            {#each data.top_databases as database}<li>
                <div>
                  <strong>{database.name}</strong><span>{metricNumber(database.total_queries)}</span
                  >
                </div>
                <div class="ranking-track" aria-hidden="true">
                  <span
                    style:width={`${(database.total_queries / data.top_databases[0].total_queries) * 100}%`}
                  ></span>
                </div>
                <p>
                  {metricNumber(database.successful_queries)} sucesso · {metricNumber(
                    database.failed_queries
                  )} falha · {metricNumber(database.denied_queries)} negado
                </p>
              </li>{/each}
          </ol>{/if}
      </article>
    </div>
    <div class="metrics-bottom-grid">
      <article class="panel errors-panel">
        <div class="panel-heading">
          <div>
            <h3>Falhas e bloqueios mais frequentes</h3>
            <p class="section-description">Até 5 combinações de erro e resultado.</p>
          </div>
        </div>
        {#if data.top_errors.length === 0}<p class="ranking-empty">
            {summary.total_queries === 0
              ? 'Sem consultas para analisar neste período.'
              : 'Nenhuma falha ou negação registrada neste período.'}
          </p>{:else}<ul class="error-ranking">
            {#each data.top_errors as error}<li>
                <div>
                  <strong>{metricErrorLabel(error.error_code)}</strong><code
                    >{error.error_code}</code
                  >
                </div>
                <span
                  class="badge"
                  class:badge-error={error.outcome === 'failure'}
                  class:badge-denied={error.outcome === 'denied'}
                  >{error.outcome === 'failure' ? 'Falha' : 'Negado'}</span
                ><strong>{metricNumber(error.count)}</strong>
              </li>{/each}
          </ul>{/if}
      </article>
      <aside class="metrics-method">
        <h3>Como ler estes números</h3>
        <p>Sucesso = consultas concluídas ÷ tentativas registradas, incluindo falhas e negações.</p>
        <p>
          O p95 é o limite de duração para 95% das medições de sucesso, incluindo gateway, Vault e
          revogação da credencial. Não é só tempo de SQL.
        </p>
        <p>
          {metricNumber(summary.timed_successful_queries)} medições de sucesso nesta amostra. A auditoria
          não é um contador garantido de todas as requisições.
        </p>
        {#if data.scope === 'workspace'}<a class="text-link" href="/audit"
            >Investigar na auditoria <Icon name="arrow" size={16} /></a
          >{/if}
      </aside>
    </div>
    {#if data.inventory}
      {@const inventory = data.inventory}
      <section class="inventory-overview" aria-labelledby="inventory-title">
        <div class="metrics-heading">
          <div>
            <h3 id="inventory-title">Agora no workspace</h3>
            <p class="section-description">
              Configuração atual. Estes números não variam com o período de atividade.
            </p>
          </div>
        </div>
        <dl class="inventory-grid">
          <div>
            <dt>Bancos ativos</dt>
            <dd>{metricNumber(inventory.active_databases)}</dd>
          </div>
          <div>
            <dt>Bancos pausados</dt>
            <dd>{metricNumber(inventory.paused_databases)}</dd>
          </div>
          <div>
            <dt>Membros habilitados</dt>
            <dd>{metricNumber(inventory.enabled_members)}</dd>
          </div>
          <div>
            <dt>Concessões efetivas</dt>
            <dd>{metricNumber(inventory.active_grants)}</dd>
          </div>
          <div>
            <dt>Ativos sem TLS</dt>
            <dd class:metric-warning={inventory.tls_disabled_databases > 0}>
              {metricNumber(inventory.tls_disabled_databases)}
            </dd>
          </div>
          <div>
            <dt>Membros sem acesso</dt>
            <dd>{metricNumber(inventory.members_without_access)}</dd>
          </div>
        </dl>
        <p class="microcopy">
          Concessões efetivas ligam identidades habilitadas a bancos ativos. Membros sem acesso não
          têm concessão a um banco ativo.
        </p>
      </section>
    {/if}
  {/if}
</section>
