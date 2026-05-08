import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DETECT_PATH = ROOT / '.graphify_detect.json'
UNCACHED_PATH = ROOT / '.graphify_uncached.txt'
OUTPUT_PATH = ROOT / '.graphify_semantic_new.json'


def node(node_id, label, file_type, source_file, source_location=None):
    return {
        'id': node_id,
        'label': label,
        'file_type': file_type,
        'source_file': source_file,
        'source_location': source_location,
        'source_url': None,
        'captured_at': None,
        'author': None,
        'contributor': None,
    }


def edge(source, target, relation, confidence, score, source_file, source_location=None, weight=1.0):
    return {
        'source': source,
        'target': target,
        'relation': relation,
        'confidence': confidence,
        'confidence_score': score,
        'source_file': source_file,
        'source_location': source_location,
        'weight': weight,
    }


def load_uncached_files():
    if UNCACHED_PATH.exists():
        return [line.strip() for line in UNCACHED_PATH.read_text(encoding='utf-8').splitlines() if line.strip()]
    if not DETECT_PATH.exists():
        return []
    detect = json.loads(DETECT_PATH.read_text(encoding='utf-8'))
    files = []
    for values in detect.get('files', {}).values():
        files.extend(values)
    return files


def build_shared_doc_graph():
    nodes = [
        node('project_kunbox', 'KunBox', 'document', 'README.md', 'README.md:1'),
        node('concept_sing_box_client', 'sing-box Proxy Client', 'document', 'README.md', 'README.md:7'),
        node('concept_tauri_react_stack', 'Tauri React Stack', 'document', 'README.md', 'README.md:23'),
        node('concept_profile_management', 'Profile Management', 'document', 'README.md', 'README.md:12'),
        node('concept_node_management', 'Node Management', 'document', 'README.md', 'README.md:13'),
        node('concept_rule_management', 'Rule Management', 'document', 'README.md', 'README.md:14'),
        node('concept_tun_mode', 'TUN Mode', 'document', 'README.md', 'README.md:17'),
        node('audit_kunbox', 'KunBox Audit Findings', 'document', 'AUDIT_REPORT.md', 'AUDIT_REPORT.md:1'),
        node('issue_cache_clear_path', 'Wrong Cache Clear Path', 'document', 'AUDIT_REPORT.md', 'AUDIT_REPORT.md:44'),
        node('issue_stale_latency_cache', 'Stale Persisted Latency Cache', 'document', 'AUDIT_REPORT.md', 'AUDIT_REPORT.md:74'),
        node('issue_settings_state_drift', 'Optimistic Settings State Drift', 'document', 'AUDIT_REPORT.md', 'AUDIT_REPORT.md:94'),
        node('concept_graphify_knowledge_graph', 'graphify Knowledge Graph', 'document', 'AGENTS.md', 'AGENTS.md:1'),
        node('concept_graph_rebuild_rule', 'Graph Rebuild Rule', 'document', 'AGENTS.md', 'AGENTS.md:5'),
        node('concept_windows_powershell', 'Windows PowerShell Workflow', 'document', 'CLAUDE.md', 'CLAUDE.md:3'),
        node('concept_frontend_backend_split', 'Frontend Backend Split', 'document', 'CLAUDE.md', 'CLAUDE.md:21'),
        node('brand_kunbox_logo', 'KunBox Logo System', 'image', 'app-icon.svg', 'app-icon.svg:1'),
        node('brand_app_icon_set', 'App Icon Asset Set', 'image', 'src-tauri\\icons\\icon.png', None),
        node('brand_android_icon_set', 'Android Launcher Icon Set', 'image', 'src-tauri\\icons\\android\\mipmap-hdpi\\ic_launcher.png', None),
        node('brand_ios_icon_set', 'iOS App Icon Set', 'image', 'src-tauri\\icons\\ios\\AppIcon-20x20@1x.png', None),
    ]
    edges = [
        edge('project_kunbox', 'concept_sing_box_client', 'references', 'EXTRACTED', 1.0, 'README.md', 'README.md:7'),
        edge('project_kunbox', 'concept_tauri_react_stack', 'references', 'EXTRACTED', 1.0, 'README.md', 'README.md:23'),
        edge('project_kunbox', 'concept_profile_management', 'references', 'EXTRACTED', 1.0, 'README.md', 'README.md:12'),
        edge('project_kunbox', 'concept_node_management', 'references', 'EXTRACTED', 1.0, 'README.md', 'README.md:13'),
        edge('project_kunbox', 'concept_rule_management', 'references', 'EXTRACTED', 1.0, 'README.md', 'README.md:14'),
        edge('project_kunbox', 'concept_tun_mode', 'references', 'EXTRACTED', 1.0, 'README.md', 'README.md:17'),
        edge('concept_tauri_react_stack', 'concept_frontend_backend_split', 'conceptually_related_to', 'INFERRED', 0.82, 'README.md', 'README.md:23'),
        edge('concept_profile_management', 'concept_node_management', 'conceptually_related_to', 'INFERRED', 0.74, 'README.md', 'README.md:12'),
        edge('concept_rule_management', 'concept_tun_mode', 'conceptually_related_to', 'INFERRED', 0.63, 'README.md', 'README.md:14'),
        edge('audit_kunbox', 'issue_cache_clear_path', 'references', 'EXTRACTED', 1.0, 'AUDIT_REPORT.md', 'AUDIT_REPORT.md:44'),
        edge('audit_kunbox', 'issue_stale_latency_cache', 'references', 'EXTRACTED', 1.0, 'AUDIT_REPORT.md', 'AUDIT_REPORT.md:74'),
        edge('audit_kunbox', 'issue_settings_state_drift', 'references', 'EXTRACTED', 1.0, 'AUDIT_REPORT.md', 'AUDIT_REPORT.md:94'),
        edge('issue_cache_clear_path', 'project_kunbox', 'rationale_for', 'INFERRED', 0.84, 'AUDIT_REPORT.md', 'AUDIT_REPORT.md:103'),
        edge('issue_stale_latency_cache', 'concept_node_management', 'conceptually_related_to', 'INFERRED', 0.77, 'AUDIT_REPORT.md', 'AUDIT_REPORT.md:78'),
        edge('issue_settings_state_drift', 'concept_frontend_backend_split', 'conceptually_related_to', 'INFERRED', 0.71, 'AUDIT_REPORT.md', 'AUDIT_REPORT.md:98'),
        edge('concept_graphify_knowledge_graph', 'concept_graph_rebuild_rule', 'references', 'EXTRACTED', 1.0, 'AGENTS.md', 'AGENTS.md:5'),
        edge('concept_graphify_knowledge_graph', 'project_kunbox', 'conceptually_related_to', 'INFERRED', 0.86, 'AGENTS.md', 'AGENTS.md:2'),
        edge('concept_graph_rebuild_rule', 'concept_frontend_backend_split', 'conceptually_related_to', 'INFERRED', 0.65, 'AGENTS.md', 'AGENTS.md:7'),
        edge('concept_windows_powershell', 'concept_frontend_backend_split', 'references', 'EXTRACTED', 1.0, 'CLAUDE.md', 'CLAUDE.md:21'),
        edge('concept_windows_powershell', 'project_kunbox', 'conceptually_related_to', 'INFERRED', 0.79, 'CLAUDE.md', 'CLAUDE.md:3'),
    ]
    hyperedges = [
        {
            'id': 'core_feature_set',
            'label': 'Core Feature Set',
            'nodes': ['concept_profile_management', 'concept_node_management', 'concept_rule_management', 'concept_tun_mode'],
            'relation': 'participate_in',
            'confidence': 'INFERRED',
            'confidence_score': 0.82,
            'source_file': 'README.md',
        },
        {
            'id': 'audit_risk_cluster',
            'label': 'Audit Risk Cluster',
            'nodes': ['issue_cache_clear_path', 'issue_stale_latency_cache', 'issue_settings_state_drift'],
            'relation': 'form',
            'confidence': 'INFERRED',
            'confidence_score': 0.79,
            'source_file': 'AUDIT_REPORT.md',
        },
        {
            'id': 'icon_platform_family',
            'label': 'Platform Icon Family',
            'nodes': ['brand_kunbox_logo', 'brand_app_icon_set', 'brand_android_icon_set', 'brand_ios_icon_set'],
            'relation': 'form',
            'confidence': 'INFERRED',
            'confidence_score': 0.87,
            'source_file': 'app-icon.svg',
        },
    ]
    return nodes, edges, hyperedges


def build_icon_nodes(uncached_files):
    nodes = []
    edges = []
    for rel in uncached_files:
        lower = rel.lower()
        if not lower.endswith(('.png', '.jpg', '.jpeg', '.webp', '.svg')):
            continue
        stem = Path(rel).stem.replace('@', '_').replace('-', '_').replace('.', '_')
        file_id = f'asset_{stem}'
        label = Path(rel).name
        nodes.append(node(file_id, label, 'image', rel, None))
        if 'android' in lower:
            parent = 'brand_android_icon_set'
            score = 0.88
        elif 'ios' in lower:
            parent = 'brand_ios_icon_set'
            score = 0.88
        else:
            parent = 'brand_app_icon_set'
            score = 0.84
        edges.append(edge(file_id, parent, 'conceptually_related_to', 'INFERRED', score, rel, None))
        edges.append(edge(file_id, 'brand_kunbox_logo', 'semantically_similar_to', 'INFERRED', 0.91 if rel == 'app-icon.svg' else 0.76, rel, None))
    return nodes, edges


def main():
    uncached_files = load_uncached_files()
    nodes, edges, hyperedges = build_shared_doc_graph()
    icon_nodes, icon_edges = build_icon_nodes(uncached_files)
    nodes.extend(icon_nodes)
    edges.extend(icon_edges)

    result = {
        'nodes': nodes,
        'edges': edges,
        'hyperedges': hyperedges,
        'input_tokens': 0,
        'output_tokens': 0,
    }
    OUTPUT_PATH.write_text(json.dumps(result, indent=2, ensure_ascii=False), encoding='utf-8')
    print(f"Semantic fallback: {len(nodes)} nodes, {len(edges)} edges, {len(hyperedges)} hyperedges")


if __name__ == '__main__':
    main()
