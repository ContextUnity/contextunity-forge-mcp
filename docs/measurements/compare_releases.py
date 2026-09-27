import argparse, hashlib, json, os, pathlib, platform, sqlite3, statistics, subprocess
parser = argparse.ArgumentParser()
parser.add_argument('--baseline', required=True)
parser.add_argument('--candidate')
parser.add_argument('--output', required=True)
parser.add_argument('--repository', help='Benchmark an existing repository instead of generated fixtures')
args = parser.parse_args()
base = pathlib.Path(args.output).resolve(); base.mkdir(parents=True, exist_ok=True)
fixtures = {}
if args.repository:
 fixtures['repository']=(pathlib.Path(args.repository).resolve(),None,None)
else:
 builders = {
  'python': ('py', lambda n:f'def f{n}(): return {n}\n'),
  'typescript': ('ts', lambda n:f'export function f{n}(): number {{ return {n}; }}\n'),
  'javascript': ('js', lambda n:f'export function f{n}() {{ return {n}; }}\n'),
  'rust': ('rs', lambda n:f'pub fn f{n}() -> usize {{ {n} }}\n'),
  'go': ('go', lambda n:f'func F{n}() int {{ return {n} }}\n'),
  'proto': ('proto', lambda n:f'message M{n} {{ string value = 1; }}\n'),
  'vue': ('vue', lambda n:f'function f{n}() {{ return {n}; }}\n')}
 for lang,(ext,fn) in builders.items():
  root=base/lang; root.mkdir(exist_ok=True)
  for i in range(100):
   source=''.join(fn(i*10+j) for j in range(10))
   if lang=='go': source='package bench\n'+source
   if lang=='proto': source='syntax = "proto3";\n'+source
   if lang=='vue': source='<script setup lang="ts">\n'+source+'</script>\n'
   (root/f'f{i}.{ext}').write_text(source)
  fixtures[lang]=(root,100,1000)
 root=base/'large_ts'; root.mkdir(exist_ok=True)
 (root/'large.ts').write_text(''.join(f'function f{i}() {{ return {i}; }}\n' for i in range(4000)))
 fixtures['large_ts']=(root,1,4000)
binaries={'baseline':str(pathlib.Path(args.baseline).resolve())}
if args.candidate: binaries['candidate']=str(pathlib.Path(args.candidate).resolve())
report={'platform':platform.platform(),'cpu_count':os.cpu_count(),'rayon_threads':4,'warmup':1,'samples':5,
 'binaries':{k:{'path':v,'sha256':hashlib.sha256(pathlib.Path(v).read_bytes()).hexdigest()} for k,v in binaries.items()},'fixtures':{}}
for name,(root,files,declarations) in fixtures.items():
 samples={key:[] for key in binaries}; snapshots={}
 for sample in range(6):
  ordering=list(binaries)
  if sample%2: ordering.reverse()
  for key in ordering:
   output=(base if args.repository else root/'.forge')/f'{key}.sqlite'
   proc=subprocess.run([binaries[key],'build',str(root),'--output',str(output)],capture_output=True,text=True,
     env={**os.environ,'RAYON_NUM_THREADS':'4'},timeout=120)
   if proc.returncode: raise RuntimeError(proc.stderr)
   values=json.loads(proc.stdout)
   if sample: samples[key].append({k:values[k] for k in ['extract_ms','link_ms','persist_ms','elapsed_ms','files','nodes','edges']})
   if sample==5:
    with sqlite3.connect(output) as conn:
     snapshots[key]={table:list(conn.execute(query)) for table,query in {
       'inventory':'SELECT path,digest FROM source_inventory ORDER BY path',
       'nodes':'SELECT id,kind,name,qualname,path,line,end_line,is_test,language,generated FROM nodes ORDER BY id',
       'edges':'SELECT src_public_id,dst_public_id,kind,path,line,confidence,occurrence_count FROM edges ORDER BY src_public_id,dst_public_id,kind',
       'coverage':'SELECT path,line,expression,status FROM resolution_coverage ORDER BY path,line,expression,status'}.items()}
 record={'files':files if files is not None else samples['baseline'][-1]['files'],'declarations':declarations,'samples':samples,'medians':{k:{metric:statistics.median(s[metric] for s in rows) for metric in rows[0]} for k,rows in samples.items()}}
 if 'candidate' in snapshots:
  record['inputs_equal']=snapshots['baseline']['inventory']==snapshots['candidate']['inventory']
  record['graph_equal']={k:snapshots['baseline'][k]==snapshots['candidate'][k] for k in ('nodes','edges','coverage')}
 report['fixtures'][name]=record
 (base/'results.json').write_text(json.dumps(report,indent=2))
 print(name,json.dumps({'medians':record['medians'],'graph_equal':record.get('graph_equal')}),flush=True)
print(base/'results.json')
