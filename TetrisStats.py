import sys
import os
import json
import colorsys
import hashlib
from PyQt5.QtWidgets import (QApplication, QMainWindow, QWidget, QVBoxLayout, QHBoxLayout, 
                             QListWidget, QPushButton, QLabel, QComboBox, QFileDialog, 
                             QSplitter, QTableWidget, QTableWidgetItem,
                             QAbstractItemView, QTabWidget, QLineEdit, QScrollArea, QDialog, QFormLayout, QDoubleSpinBox,
                             QMessageBox, QProgressDialog)
from PyQt5.QtCore import Qt
from PyQt5.QtGui import QPainter, QColor, QPen, QFont
import numpy as np
import concurrent.futures

# Define the ranges for each statistic
STAT_RANGES = {
    'PPS': (0, 4),
    'APM': (0, 240),
    'VS Score': (0, 400),
    'APP': (0, 1),
    'DS/Piece': (0, 0.5),
    'DS/Second': (0, 1),
    'Garbage Efficiency': (0, 0.6),
    'Damage Potential': (0, 8)
}

def generate_distinct_colors(n):
    colors = []
    for i in range(n):
        hue = i / n
        saturation = 0.7
        value = 0.9
        rgb = colorsys.hsv_to_rgb(hue, saturation, value)
        colors.append(QColor(int(rgb[0]*255), int(rgb[1]*255), int(rgb[2]*255)))
    return colors

def normalize_stat(value, stat_name):
    min_val, max_val = STAT_RANGES[stat_name]
    if max_val == min_val:
        return 0.5
    return min(max((value - min_val) / (max_val - min_val), 0), 1)

def calculate_garbage_efficiency(app, ds_per_piece):
    if app <= 0 or ds_per_piece <= 0:
        return 0
    return (app * ds_per_piece) * 2

def calculate_app(apm, pps):
    if pps <= 0 or apm <= 0:
        return 0
    return apm / (pps * 60)

def calculate_ds_per_piece(vs, apm, pps):
    if pps <= 0 or apm <= 0:
        return 0
    ds_per_second = (vs / 100) - (apm / 60)
    return ds_per_second / pps

def calculate_ds_per_second(vs, apm):
    return (vs / 100) - (apm / 60)

def calculate_damage_potential(pps, app, ge):
    return pps * (1 + app) * (1 + ge)

def build_cache_file_path(file_path, cache_dir):
    normalized_path = os.path.normcase(os.path.abspath(file_path))
    cache_key = hashlib.sha256(normalized_path.encode('utf-8')).hexdigest()
    return os.path.join(cache_dir, f"{cache_key}.cache")

def get_file_signature(file_path):
    file_stats = os.stat(file_path)
    return {
        'path': os.path.abspath(file_path),
        'size': file_stats.st_size,
        'mtime_ns': file_stats.st_mtime_ns
    }

def load_cached_result(file_path, cache_dir):
    cache_file = build_cache_file_path(file_path, cache_dir)
    if not os.path.exists(cache_file):
        return None

    try:
        with open(cache_file, 'r', encoding='utf-8') as cache_handle:
            cached_data = json.load(cache_handle)
    except (OSError, json.JSONDecodeError):
        return None

    if not isinstance(cached_data, dict):
        return None

    if cached_data.get('source') != get_file_signature(file_path):
        return None

    result = cached_data.get('result')
    if not isinstance(result, list) or len(result) != 3:
        return None

    return tuple(result)

class PlayerProfile:
    def __init__(self, username):
        self.username = username
        self.games_played = 0
        self.stats = {
            'PPS': [],
            'APM': [],
            'VS Score': [],
            'APP': [],
            'DS/Piece': [],
            'DS/Second': [],
            'Garbage Efficiency': [],
            'Damage Potential': []
        }
        self.personal_bests = {stat: 0 for stat in self.stats}
        self.matchups = {}

    def add_game(self, game_stats):
        self.games_played += 1
        for stat, value in game_stats.items():
            self.stats[stat].append(value)
            if value > self.personal_bests[stat]:
                self.personal_bests[stat] = value

    def get_averages(self):
        return {stat: sum(values) / len(values) if values else 0 for stat, values in self.stats.items()}

    def get_personal_bests(self):
        return self.personal_bests

    def add_matchup(self, opponent, result):
        if opponent not in self.matchups:
            self.matchups[opponent] = {'wins': 0, 'losses': 0}
        if result == 'win':
            self.matchups[opponent]['wins'] += 1
        else:
            self.matchups[opponent]['losses'] += 1

    def get_matchup_history(self):
        return {opponent: {'ratio': wins['wins'] / (wins['wins'] + wins['losses']) if (wins['wins'] + wins['losses']) > 0 else 0, 
                           'total_games': wins['wins'] + wins['losses']}
                for opponent, wins in self.matchups.items()}

def process_file(file_path, cache_dir, force_reprocess=False):
    try:
        os.makedirs(cache_dir, exist_ok=True)
        cache_file = build_cache_file_path(file_path, cache_dir)

        if not force_reprocess:
            cached_result = load_cached_result(file_path, cache_dir)
            if cached_result is not None:
                return cached_result

        with open(file_path, 'r', encoding='utf-8') as replay_file:
            data = json.load(replay_file)

        round_stats = []
        overall_stats = {}
        winner = None

        replay_data = data.get('replay')
        if isinstance(replay_data, dict):
            # Determine the winner
            leaderboard = replay_data.get('leaderboard', [])
            if leaderboard:
                winner = max(leaderboard, key=lambda x: x['wins'])['username']

            for round_data in replay_data.get('rounds', []):
                round_result = {}
                for player_data in round_data:
                    stats = player_data['stats']
                    username = player_data['username']
                    pps = stats['pps']
                    apm = stats['apm']
                    vs = stats['vsscore']
                    app = calculate_app(apm, pps)
                    ds_per_piece = calculate_ds_per_piece(vs, apm, pps)
                    ds_per_second = calculate_ds_per_second(vs, apm)
                    garbage_efficiency = calculate_garbage_efficiency(app, ds_per_piece)
                    damage_potential = calculate_damage_potential(pps, app, garbage_efficiency)

                    round_result[username] = {
                        'PPS': pps,
                        'APM': apm,
                        'VS Score': vs,
                        'APP': app,
                        'DS/Piece': ds_per_piece,
                        'DS/Second': ds_per_second,
                        'Garbage Efficiency': garbage_efficiency,
                        'Damage Potential': damage_potential
                    }

                    if username not in overall_stats:
                        overall_stats[username] = {stat: [] for stat in round_result[username]}

                    for stat, value in round_result[username].items():
                        overall_stats[username][stat].append(value)

                if round_result:
                    round_stats.append(round_result)

        else:
            raise ValueError("Unknown replay format")

        for username in overall_stats:
            for stat in overall_stats[username]:
                overall_stats[username][stat] = sum(overall_stats[username][stat]) / len(overall_stats[username][stat])

        result = (round_stats, overall_stats, winner)

        with open(cache_file, 'w', encoding='utf-8') as cache_handle:
            json.dump({
                'source': get_file_signature(file_path),
                'result': result
            }, cache_handle)
        
        return result
    except Exception as e:
        print(f"Error processing file {file_path}: {str(e)}")
        return None

class RadarChart(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.stats = {}
        self.players = []
        self.stat_names = ['PPS', 'APM', 'VS Score', 'APP', 'DS/Piece', 'DS/Second', 'Garbage Efficiency']

    def set_data(self, stats):
        self.stats = stats
        self.players = list(stats.keys())
        self.update()

    def paintEvent(self, event):
        if not self.stats:
            return

        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)

        width = self.width()
        height = self.height()
        center_x = width / 2
        center_y = height / 2
        radius = min(width, height) / 2 - 60

        angles = np.linspace(0, 2*np.pi, len(self.stat_names), endpoint=False)

        painter.setPen(QPen(QColor(100, 100, 100), 1))
        for angle in angles:
            x = center_x + radius * np.cos(angle)
            y = center_y + radius * np.sin(angle)
            painter.drawLine(int(center_x), int(center_y), int(x), int(y))

        painter.setPen(QColor(200, 200, 200))
        for i, stat in enumerate(self.stat_names):
            angle = angles[i]
            x = center_x + (radius + 30) * np.cos(angle)
            y = center_y + (radius + 30) * np.sin(angle)
            
            flags = Qt.AlignCenter
            if x < center_x:
                flags |= Qt.AlignRight
            elif x > center_x:
                flags |= Qt.AlignLeft
            if y < center_y:
                flags |= Qt.AlignBottom
            elif y > center_y:
                flags |= Qt.AlignTop
            
            rect = painter.boundingRect(int(x-50), int(y-10), 100, 20, flags, stat)
            painter.drawText(rect, flags, stat)

        colors = generate_distinct_colors(len(self.players))
        for i, (player, player_stats) in enumerate(self.stats.items()):
            painter.setPen(QPen(colors[i], 2))
            points = []
            for j, stat in enumerate(self.stat_names):
                value = normalize_stat(player_stats[stat], stat)
                x = center_x + radius * value * np.cos(angles[j])
                y = center_y + radius * value * np.sin(angles[j])
                points.append((int(x), int(y)))
            
            for j in range(len(points)):
                painter.drawLine(points[j][0], points[j][1], points[(j+1)%len(points)][0], points[(j+1)%len(points)][1])

        self.draw_legend(painter)

    def draw_legend(self, painter):
        colors = generate_distinct_colors(len(self.players))
        legend_x = 10
        legend_y = self.height() - 30
        
        for i, player in enumerate(self.players):
            painter.setPen(QPen(colors[i], 2))
            painter.setBrush(colors[i])
            painter.drawRect(legend_x, legend_y, 20, 20)
            painter.drawText(legend_x + 25, legend_y + 15, player)
            legend_x += 175

class ManualInputDialog(QDialog):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Manual Stat Input")
        layout = QVBoxLayout(self)
        
        form_layout = QFormLayout()
        
        self.pps_input = QDoubleSpinBox()
        self.pps_input.setRange(0, 10)
        self.pps_input.setDecimals(2)
        self.pps_input.setSingleStep(0.1)
        self.pps_input.setToolTip("Typical range: 0.1 - 4.5")
        
        self.apm_input = QDoubleSpinBox()
        self.apm_input.setRange(0, 500)
        self.apm_input.setDecimals(2)
        self.apm_input.setSingleStep(1)
        self.apm_input.setToolTip("Typical range: 1 - 350")
        
        self.vs_input = QDoubleSpinBox()
        self.vs_input.setRange(0, 1000)
        self.vs_input.setDecimals(2)
        self.vs_input.setSingleStep(1)
        self.vs_input.setToolTip("Typical range: 1 - 500")
        
        form_layout.addRow("PPS:", self.pps_input)
        form_layout.addRow("APM:", self.apm_input)
        form_layout.addRow("VS Score:", self.vs_input)
        
        layout.addLayout(form_layout)
        
        submit_button = QPushButton("Submit")
        submit_button.clicked.connect(self.accept)
        layout.addWidget(submit_button)
        
    def get_values(self):
        return {
            'PPS': self.pps_input.value(),
            'APM': self.apm_input.value(),
            'VS Score': self.vs_input.value()
        }

class AttackDefenseSpeedChart(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.stats = {}
        self.players = []
        self.stat_names = ['APP', 'Garbage Efficiency', 'PPS', 'Damage Potential']
        self.display_names = ['Attack Power', 'Defense/Boardstate', 'Speed', 'Damage Potential']

    def set_data(self, stats):
        self.stats = stats
        self.players = list(stats.keys())
        self.update()

    def paintEvent(self, event):
        if not self.stats:
            return

        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)

        width = self.width()
        height = self.height()
        center_x = width / 2
        center_y = height / 2
        radius = min(width, height) / 2 - 60

        angles = np.linspace(0, 2*np.pi, len(self.stat_names), endpoint=False)

        painter.setPen(QPen(QColor(100, 100, 100), 1))
        for angle in angles:
            x = center_x + radius * np.cos(angle)
            y = center_y + radius * np.sin(angle)
            painter.drawLine(int(center_x), int(center_y), int(x), int(y))

        painter.setPen(QColor(200, 200, 200))
        for i, display_name in enumerate(self.display_names):
            angle = angles[i]
            x = center_x + (radius + 30) * np.cos(angle)
            y = center_y + (radius + 30) * np.sin(angle)
            
            flags = Qt.AlignCenter
            if x < center_x:
                flags |= Qt.AlignRight
            elif x > center_x:
                flags |= Qt.AlignLeft
            if y < center_y:
                flags |= Qt.AlignBottom
            elif y > center_y:
                flags |= Qt.AlignTop
            
            rect = painter.boundingRect(int(x-50), int(y-10), 100, 20, flags, display_name)
            painter.drawText(rect, flags, display_name)

        colors = generate_distinct_colors(len(self.players))
        for i, (player, player_stats) in enumerate(self.stats.items()):
            painter.setPen(QPen(colors[i], 2))
            points = []
            for j, stat in enumerate(self.stat_names):
                value = normalize_stat(player_stats[stat], stat)
                x = center_x + radius * value * np.cos(angles[j])
                y = center_y + radius * value * np.sin(angles[j])
                points.append((int(x), int(y)))
            
            for j in range(len(points)):
                painter.drawLine(points[j][0], points[j][1], points[(j+1)%len(points)][0], points[(j+1)%len(points)][1])

        self.draw_legend(painter)

    def draw_legend(self, painter):
        colors = generate_distinct_colors(len(self.players))
        legend_x = 10
        legend_y = self.height() - 30
        
        for i, player in enumerate(self.players):
            painter.setPen(QPen(colors[i], 2))
            painter.setBrush(colors[i])
            painter.drawRect(legend_x, legend_y, 20, 20)
            painter.drawText(legend_x + 25, legend_y + 15, player)
            legend_x += 175

class PlayerStatsWidget(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.layout = QVBoxLayout(self)
        self.layout.setContentsMargins(0, 0, 0, 0)
        self.layout.setSpacing(0)
        self.scroll_area = QScrollArea()
        self.scroll_area.setWidgetResizable(True)
        self.scroll_content = QWidget()
        self.scroll_layout = QVBoxLayout(self.scroll_content)
        self.scroll_area.setWidget(self.scroll_content)
        self.layout.addWidget(self.scroll_area)

    def update_stats(self, stats, winner=None):
        for i in reversed(range(self.scroll_layout.count())): 
            self.scroll_layout.itemAt(i).widget().setParent(None)

        if not stats:
            empty_label = QLabel("No data to display")
            empty_label.setAlignment(Qt.AlignCenter)
            self.scroll_layout.addWidget(empty_label)
            return

        players = list(stats.keys())
        stat_names = ['PPS', 'APM', 'VS Score', 'APP', 'DS/Piece', 'DS/Second', 'Garbage Efficiency']

        table = QTableWidget(len(stat_names) + 2, len(players) + 1)
        table.setVerticalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        table.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        table.horizontalHeader().setVisible(False)
        table.verticalHeader().setVisible(False)

        table.setColumnWidth(0, 250)
        for col in range(1, len(players) + 1):
            table.setColumnWidth(col, 250)

        for row in range(table.rowCount()):
            table.setRowHeight(row, 30)

        colors = generate_distinct_colors(len(stats))
        for col, player in enumerate(players, start=1):
            item = QTableWidgetItem(player)
            item.setTextAlignment(Qt.AlignCenter)
            item.setBackground(colors[col-1])
            item.setForeground(QColor(255, 255, 255))
            font = item.font()
            font.setBold(True)
            font.setPointSize(12)
            item.setFont(font)
            table.setItem(0, col, item)

        for row, stat in enumerate(stat_names, start=1):
            item = QTableWidgetItem(stat)
            item.setTextAlignment(Qt.AlignCenter)
            font = item.font()
            font.setPointSize(11)
            item.setFont(font)
            table.setItem(row, 0, item)

            for col, player in enumerate(players, start=1):
                value = stats[player][stat]
                item = QTableWidgetItem(f"{value:.2f}")
                item.setTextAlignment(Qt.AlignCenter)
                font = item.font()
                font.setPointSize(11)
                item.setFont(font)
                table.setItem(row, col, item)

        winner_row = len(stat_names) + 1
        for col, player in enumerate(players, start=1):
            item = QTableWidgetItem("WINNER" if player == winner else "")
            item.setTextAlignment(Qt.AlignCenter)
            item.setBackground(QColor('#4CAF50') if player == winner else QColor('#2b2b2b'))
            item.setForeground(QColor(255, 255, 255))
            font = item.font()
            font.setBold(True)
            font.setPointSize(12)
            item.setFont(font)
            table.setItem(winner_row, col, item)

        self.scroll_layout.addWidget(table)

        self.setStyleSheet("""
            QTableWidget { 
                background-color: #2b2b2b; 
                color: #ffffff; 
                gridline-color: #3a3a3a;
            }
            QTableWidget::item { 
                padding: 5px; 
            }
        """)

        self.update()

class ReplayAnalyzer(QMainWindow):
    def __init__(self):
        super().__init__()

        self.setWindowTitle("Tetr.io Replay Analyzer")
        self.setGeometry(100, 100, 1920, 1080)
        self.setStyleSheet("""
            QMainWindow, QWidget { background-color: #2b2b2b; color: #ffffff; }
            QTableWidget { gridline-color: #3a3a3a; }
            QHeaderView::section { background-color: #3a3a3a; }
            QComboBox, QPushButton { background-color: #3a3a3a; border: 1px solid #505050; padding: 5px; }
            QListWidget { background-color: #323232; border: 1px solid #505050; }
            QListWidget::item:selected { background-color: #4a4a4a; }
            QTabBar::tab { background-color: #3a3a3a; color: #ffffff; padding: 8px; }
            QTabBar::tab:selected { background-color: #4a4a4a; }
            QLineEdit { background-color: #3a3a3a; color: #ffffff; border: 1px solid #505050; padding: 5px; }
        """)
        self.cache_dir = os.path.join(os.path.dirname(os.path.abspath(__file__)), "replay_cache")
        os.makedirs(self.cache_dir, exist_ok=True)
        self.central_widget = QWidget()
        self.setCentralWidget(self.central_widget)
        self.layout = QHBoxLayout(self.central_widget)

        self.main_splitter = QSplitter(Qt.Horizontal)
        self.layout.addWidget(self.main_splitter)

        self.all_game_data = {}
        self.current_analysis = None
        self.current_file = None
        self.current_folder = None
        self.player_profiles = {}

        self.create_file_browser()
        self.create_stats_view()

        self.main_splitter.setSizes([200, 1000])
        self.clear_analysis_view()

    def create_large_font(self):
        font = QFont()
        font.setPointSize(12)
        return font

    def set_current_analysis(self, overall_stats, winner=None, round_stats=None, current_file=None):
        self.current_analysis = {
            'round_stats': round_stats or [],
            'overall_stats': overall_stats or {},
            'winner': winner
        }
        self.current_file = current_file

        round_items = [f"Round {i+1}" for i in range(len(self.current_analysis['round_stats']))]
        round_items.append("Average")

        self.round_selector.blockSignals(True)
        self.round_selector.clear()
        self.round_selector.addItems(round_items)
        self.round_selector.setCurrentIndex(len(round_items) - 1)
        self.round_selector.setEnabled(True)
        self.round_selector.blockSignals(False)

    def clear_analysis_view(self):
        self.current_analysis = None
        self.current_file = None

        self.round_selector.blockSignals(True)
        self.round_selector.clear()
        self.round_selector.setEnabled(False)
        self.round_selector.blockSignals(False)

        self.update_stats_display({})
        self.update_graphs({})

    def filter_stats(self, stats):
        filter_text = self.player_filter.text().strip().lower()
        if not filter_text:
            return stats

        return {
            player: player_stats
            for player, player_stats in stats.items()
            if filter_text in player.lower()
        }

    def apply_current_view(self):
        if not self.current_analysis:
            self.update_stats_display({})
            self.update_graphs({})
            return

        round_stats = self.current_analysis['round_stats']
        overall_stats = self.current_analysis['overall_stats']
        winner = self.current_analysis['winner']
        round_index = self.round_selector.currentIndex()

        if not round_stats or round_index < 0 or round_index >= len(round_stats):
            display_stats = overall_stats
            display_winner = winner
        else:
            display_stats = round_stats[round_index]
            display_winner = max(display_stats, key=lambda player: display_stats[player]['VS Score']) if display_stats else None

        filtered_stats = self.filter_stats(display_stats)
        if display_winner not in filtered_stats:
            display_winner = None

        self.update_stats_display(filtered_stats, display_winner)
        self.update_graphs(filtered_stats)

    def manual_input(self):
        dialog = ManualInputDialog(self)
        if dialog.exec_():
            manual_stats = dialog.get_values()

            pps = manual_stats['PPS']
            apm = manual_stats['APM']
            vs = manual_stats['VS Score']

            app = calculate_app(apm, pps)
            ds_per_piece = calculate_ds_per_piece(vs, apm, pps)
            ds_per_second = calculate_ds_per_second(vs, apm)
            garbage_efficiency = calculate_garbage_efficiency(app, ds_per_piece)
            damage_potential = calculate_damage_potential(pps, app, garbage_efficiency)

            manual_stats.update({
                'APP': app,
                'DS/Piece': ds_per_piece,
                'DS/Second': ds_per_second,
                'Garbage Efficiency': garbage_efficiency,
                'Damage Potential': damage_potential
            })

            self.clear_player_profiles()
            self.set_current_analysis({'Manual Input': manual_stats})
            self.apply_current_view()
            self.update_player_profiles({'Manual Input': manual_stats})
            self.update_player_profiles_display()

    def create_file_browser(self):
        file_frame = QWidget()
        file_layout = QVBoxLayout(file_frame)

        self.file_list = QListWidget()
        self.file_list.setSelectionMode(QAbstractItemView.ExtendedSelection)
        self.file_list.itemSelectionChanged.connect(self.on_file_selection_changed)

        select_button = QPushButton("Select Folder")
        select_button.clicked.connect(self.select_folder)

        refresh_button = QPushButton("Refresh")
        refresh_button.clicked.connect(self.refresh_files)

        analyze_button = QPushButton("Analyze Selected")
        analyze_button.clicked.connect(self.analyze_selected_files)

        reprocess_button = QPushButton("Rebuild Cache")
        reprocess_button.clicked.connect(self.reprocess_all_files)

        manual_input_button = QPushButton("Manual Input")
        manual_input_button.clicked.connect(self.manual_input)

        button_layout = QHBoxLayout()
        button_layout.addWidget(select_button)
        button_layout.addWidget(refresh_button)
        button_layout.addWidget(analyze_button)
        button_layout.addWidget(reprocess_button)
        button_layout.addWidget(manual_input_button)

        file_layout.addWidget(QLabel("Replay Files"))
        file_layout.addWidget(self.file_list)
        file_layout.addLayout(button_layout)

        self.main_splitter.addWidget(file_frame)

    def create_stats_view(self):
        stats_frame = QWidget()
        stats_layout = QVBoxLayout(stats_frame)
    
        self.player_filter = QLineEdit()
        self.player_filter.setPlaceholderText("Filter players...")
        self.player_filter.textChanged.connect(self.filter_players)
    
        self.round_selector = QComboBox()
        self.round_selector.currentIndexChanged.connect(self.on_round_select)
    
        self.player_stats_widget = PlayerStatsWidget()
    
        self.radar_chart = RadarChart()
        self.attack_defense_speed_chart = AttackDefenseSpeedChart()
    
        charts_splitter = QSplitter(Qt.Horizontal)
        charts_splitter.addWidget(self.radar_chart)
        charts_splitter.addWidget(self.attack_defense_speed_chart)
    
        content_splitter = QSplitter(Qt.Vertical)
        content_splitter.addWidget(self.player_stats_widget)
        content_splitter.addWidget(charts_splitter)
        content_splitter.setSizes([200, 400])
    
        self.profile_tabs = QTabWidget()

        main_splitter = QSplitter(Qt.Vertical)
        main_splitter.addWidget(content_splitter)
        main_splitter.addWidget(self.profile_tabs)
        main_splitter.setSizes([400, 200])

        stats_layout.addWidget(QLabel("Replay Stats"))
        stats_layout.addWidget(self.player_filter)
        stats_layout.addWidget(self.round_selector)
        stats_layout.addWidget(main_splitter)

        self.main_splitter.addWidget(stats_frame)

    def refresh_files(self):
        if self.current_folder:
            file_names = sorted(
                file_name for file_name in os.listdir(self.current_folder)
                if file_name.endswith('.ttrm')
            )
            self.file_list.clear()
            for file_name in file_names:
                self.file_list.addItem(file_name)

            if self.current_file and self.current_file not in file_names:
                self.clear_player_profiles()
                self.clear_analysis_view()

    def select_folder(self):
        folder_path = QFileDialog.getExistingDirectory(self, "Select Folder")
        if folder_path:
            self.current_folder = folder_path
            self.all_game_data = {}
            self.clear_player_profiles()
            self.clear_analysis_view()
            self.refresh_files()

    def on_file_selection_changed(self):
        selected_items = self.file_list.selectedItems()
        if len(selected_items) == 1:
            self.on_file_select(selected_items[0])
        else:
            self.clear_player_profiles()
            self.clear_analysis_view()

    def on_file_select(self, item):
        file_name = item.text()
        file_path = os.path.join(self.current_folder, file_name)
        result = process_file(file_path, self.cache_dir)
        if result is not None:
            self.all_game_data[file_name] = result
            self.display_results(file_name)
        else:
            QMessageBox.warning(self, "Error", f"Failed to process file: {file_name}")

    def update_player_profiles(self, game_data):
        for player, stats in game_data.items():
            if player not in self.player_profiles:
                self.player_profiles[player] = PlayerProfile(player)
            self.player_profiles[player].add_game(stats)

    def clear_player_profiles(self):
        self.player_profiles = {}
        while self.profile_tabs.count() > 0:
            self.profile_tabs.removeTab(0)

    def filter_players(self):
        self.apply_current_view()

    def display_results(self, file_name):
        round_stats, overall_stats, winner = self.all_game_data[file_name]

        self.clear_player_profiles()
        self.set_current_analysis(overall_stats, winner, round_stats, current_file=file_name)
        self.apply_current_view()
        self.update_player_profiles(overall_stats)
        self.update_player_profiles_display()

    def update_player_profiles_display(self):
        while self.profile_tabs.count() > 0:
            self.profile_tabs.removeTab(0)

        large_font = self.create_large_font()

        for player, profile in self.player_profiles.items():
            tab = QWidget()
            layout = QVBoxLayout(tab)

            style = self.analyze_play_style(profile)
            style_label = QLabel(f"Play Style: {style}")
            style_label.setFont(large_font)
            layout.addWidget(style_label)

            suggestions = self.get_improvement_suggestions(profile)
            suggestions_label = QLabel("Improvement Suggestions:")
            suggestions_label.setFont(large_font)
            layout.addWidget(suggestions_label)
            for suggestion in suggestions:
                suggestion_label = QLabel(f"- {suggestion}")
                suggestion_label.setFont(large_font)
                suggestion_label.setWordWrap(True)
                layout.addWidget(suggestion_label)

            self.profile_tabs.addTab(tab, player)

        self.profile_tabs.setMaximumHeight(400)
        self.profile_tabs.setMinimumHeight(150)

    def analyze_play_style(self, player_profile):
        averages = player_profile.get_averages()
        app = averages['APP']
        vs_apm_ratio = averages['VS Score'] / averages['APM'] if averages['APM'] > 0 else 0
        ge = averages['Garbage Efficiency']
        pps = averages['PPS']

        app_thresholds = [0.3, 0.45, 0.6, 0.75, 0.9]
        ge_thresholds = [0.05, 0.10, 0.15, 0.20, 0.30]
        vs_apm_thresholds = [1.6, 1.9, 2.0, 2.2, 2.5]
        pps_thresholds = [1.0, 2.0, 2.5, 3.0, 4]
        
        def categorize(value, thresholds):
            categories = ["Low", "Below Average", "Average", "Above Average", "High", "Extremely High", "God-Tier"]
            for i, threshold in enumerate(thresholds):
                if value < threshold:
                    return categories[i]
            return categories[-1]

        app_category = categorize(app, app_thresholds)
        ge_category = categorize(ge, ge_thresholds)
        vs_apm_category = categorize(vs_apm_ratio, vs_apm_thresholds)
        pps_category = categorize(pps, pps_thresholds)

        speed_descriptors = {
            "Low": "Very low-speed",
            "Below Average": "Low-speed",
            "Average": "Medium-speed",
            "Above Average": "High-speed",
            "High": "Very high-speed",
            "Extremely High": "Extremely high-speed",
            "God-Tier": "God Tier-speed"
        }
        speed_descriptor = speed_descriptors[pps_category]

        if app_category in ["God-Tier", "Extremely High"]:
            attack_style = "Highly efficient attacker"
        elif app_category in ["High", "Above Average"]:
            attack_style = "Efficient attacker"
        elif app_category == "Average":
            attack_style = "Balanced attacker"
        else:
            attack_style = "Inefficient attacker"

        if vs_apm_category in ["Low", "Below Average"]:
            if pps_category in ["High", "Extremely High", "God-Tier"]:
                aggressiveness = "Highly offensive"
            elif pps_category in ["Above Average", "Average"]:
                aggressiveness = "Offensive"
            else:
                aggressiveness = "Low-pressure player"
        elif vs_apm_category in ["Average", "Above Average"]:
            aggressiveness = "Balanced"
        else:
            if ge_category in ["High", "Extremely High", "God-Tier"]:
                aggressiveness = "Defensive specialist"
            elif ge_category in ["Above Average", "Average"]:
                aggressiveness = "Pressure-resistant"
            else:
                aggressiveness = "Defensive struggler"

        if vs_apm_category in ["God-Tier", "Extremely High", "High"]:
            if ge_category in ["God-Tier", "Extremely High", "High"]:
                garbage_style = "Exceptional garbage handler under extreme pressure"
            elif ge_category in ["Above Average", "Average"]:
                garbage_style = "Competent garbage handler under high pressure"
            else:
                garbage_style = "Struggles with efficiency under high pressure"
        elif vs_apm_category in ["Above Average", "Average"]:
            if ge_category in ["God-Tier", "Extremely High", "High"]:
                garbage_style = "Highly efficient garbage handler under moderate pressure"
            elif ge_category in ["Above Average", "Average"]:
                garbage_style = "Balanced garbage handling under moderate pressure"
            else:
                garbage_style = "Inefficient garbage handler under moderate pressure"
        else:
            if ge_category in ["God-Tier", "Extremely High", "High"]:
                garbage_style = "Highly efficient garbage handler with low incoming pressure"
            elif ge_category in ["Above Average", "Average"]:
                garbage_style = "Competent garbage handler with low incoming pressure"
            else:
                garbage_style = "Inefficient garbage handling, even under low pressure"

        playstyle = f"{speed_descriptor}, {aggressiveness} player with {attack_style.lower()} capabilities. {garbage_style}."

        if vs_apm_category in ["God-Tier", "Extremely High", "High"] and app_category in ["God-Tier", "Extremely High", "High"]:
            playstyle += " Excels in high-pressure situations with efficient counterattacks."
        elif vs_apm_category in ["Low", "Below Average"] and pps_category in ["High", "Extremely High", "God-Tier"]:
            playstyle += " Dominates through relentless offensive pressure."
        elif vs_apm_category in ["God-Tier", "Extremely High", "High"] and ge_category in ["God-Tier", "Extremely High", "High"]:
            playstyle += " Thrives on efficient downstacking under extreme pressure."
        elif vs_apm_category in ["Low", "Below Average"] and app_category in ["High", "Extremely High", "God-Tier"]:
            playstyle += " Efficiently converts opportunities into strong attacks."

        return playstyle

    def get_improvement_suggestions(self, player_profile):
        averages = player_profile.get_averages()
        app = averages['APP']
        vs_apm_ratio = averages['VS Score'] / averages['APM'] if averages['APM'] > 0 else 0
        ge = averages['Garbage Efficiency']
        pps = averages['PPS']

        app_thresholds = [0.3, 0.45, 0.6, 0.75, 0.9]
        ge_thresholds = [0.05, 0.10, 0.15, 0.20, 0.30]
        vs_apm_thresholds = [1.6, 1.9, 2.0, 2.2, 2.5]
        pps_thresholds = [1.0, 2.0, 2.5, 3.0, 4]

        def categorize(value, thresholds):
            categories = ["Low", "Below Average", "Average", "Above Average", "High", "Extremely High", "God-Tier"]
            for i, threshold in enumerate(thresholds):
                if value < threshold:
                    return categories[i]
            return categories[-1]

        app_category = categorize(app, app_thresholds)
        ge_category = categorize(ge, ge_thresholds)
        vs_apm_category = categorize(vs_apm_ratio, vs_apm_thresholds)
        pps_category = categorize(pps, pps_thresholds)

        suggestions = []

        if pps_category == "God-Tier":
            suggestions.append("Your speed is phenomenal. Focus on maintaining this level while optimizing efficiency, attack power, and consistency under varying pressure situations.")
        elif pps_category in ["Extremely High", "High"]:
            suggestions.append("Your speed is excellent. Work on consistency and efficiency at these high speeds.")
        elif pps_category in ["Above Average", "Average"]:
            suggestions.append("Your speed is good. Continue to improve by practicing finesse and efficient piece placement.")
        else:
            suggestions.append("Focus on increasing your overall speed (PPS). Practice finesse and efficient piece placement.")

        if ge_category == "God-Tier":
            suggestions.append("Your garbage efficiency is outstanding. Maintain this level while optimizing other aspects of your game.")
        elif ge_category in ["Extremely High", "High"]:
            suggestions.append("Your garbage efficiency is very good. Fine-tune your downstacking for even better performance under pressure.")
        elif ge_category in ["Above Average", "Average"]:
            suggestions.append("Your garbage efficiency is decent. Practice more efficient downstacking techniques to improve further.")
        else:
            suggestions.append("Work on improving your garbage efficiency. Focus on cleaner downstacking and better piece placement.")

        if app_category == "God-Tier":
            suggestions.append("Your attack efficiency is incredible. Focus on maintaining this level while adapting to different board states and opponent playstyles.")
        elif app_category in ["Extremely High", "High"]:
            suggestions.append("Your attack efficiency is very good. Work on consistency and adapting to different situations.")
        elif app_category in ["Above Average", "Average"]:
            suggestions.append("Your attack efficiency is solid. Practice more advanced attack techniques to increase your APP.")
        else:
            suggestions.append("Improve your attack efficiency (APP). Practice building cleaner and executing attacks faster.")

        if vs_apm_category in ["High", "Extremely High", "God-Tier"] and app_category in ["High", "Extremely High", "God-Tier"]:
            suggestions.append("You're effectively attacking while handling high pressure. Focus on maintaining this balance and look for opportunities to overwhelm opponents.")
        elif vs_apm_category in ["High", "Extremely High", "God-Tier"] and app_category in ["Low", "Below Average", "Average"]:
            suggestions.append("You're handling high pressure but could improve your attack efficiency. Work on building and executing attacks more effectively under pressure.")
        elif vs_apm_category in ["Low", "Below Average", "Average"] and app_category in ["High", "Extremely High", "God-Tier"]:
            suggestions.append("Your attacks are highly efficient, but you're not under much pressure. Practice maintaining this efficiency against stronger opponents or in faster-paced games.")
        elif vs_apm_category in ["Low", "Below Average", "Average"] and app_category in ["Low", "Below Average", "Average"]:
            suggestions.append("You're not under much pressure, but your attacks could be more efficient. Focus on improving your offensive capabilities to control the game better.")

        if vs_apm_category in ["High", "Extremely High", "God-Tier"] and ge_category in ["High", "Extremely High", "God-Tier"]:
            suggestions.append("You're excellently managing high amounts of garbage. Work on offensive strategies to reduce incoming attacks while maintaining this efficiency.")
        elif vs_apm_category in ["High", "Extremely High", "God-Tier"] and ge_category in ["Low", "Below Average", "Average"]:
            suggestions.append("You're under high pressure and could improve your garbage management. Focus on more efficient downstacking techniques.")
        elif vs_apm_category in ["Low", "Below Average", "Average"] and ge_category in ["High", "Extremely High", "God-Tier"]:
            suggestions.append("Your garbage efficiency is high, but you're not under much pressure. Prepare for handling higher pressure situations while maintaining this efficiency.")
        elif vs_apm_category in ["Low", "Below Average", "Average"] and ge_category in ["Low", "Below Average", "Average"]:
            suggestions.append("You're not under much pressure, but could improve garbage efficiency. Work on downstacking techniques to prepare for higher-pressure games.")

        if pps_category in ["High", "Extremely High", "God-Tier"] and app_category in ["Low", "Below Average"]:
            suggestions.append("Your speed is excellent, but your attack efficiency could improve. Focus on converting your quick placements into more effective attacks.")
        elif app_category in ["High", "Extremely High", "God-Tier"] and pps_category in ["Low", "Below Average"]:
            suggestions.append("Your attack efficiency is high, but overall speed is low. Work on increasing PPS while maintaining strong attack patterns.")

        return suggestions[:5]

    def update_stats_display(self, stats, winner=None):
        self.player_stats_widget.update_stats(stats, winner)

    def update_graphs(self, stats):
        self.radar_chart.set_data(stats)
        self.radar_chart.update()
        self.attack_defense_speed_chart.set_data(stats)
        self.attack_defense_speed_chart.update()

    def on_round_select(self, index):
        self.apply_current_view()

    def analyze_selected_files(self):
        selected_items = self.file_list.selectedItems()
        if not selected_items:
            return

        self.clear_player_profiles()

        file_paths = [os.path.join(self.current_folder, item.text()) for item in selected_items]
        
        progress = QProgressDialog("Analyzing replays...", "Cancel", 0, len(file_paths), self)
        progress.setWindowModality(Qt.WindowModal)
        progress.setMinimumDuration(0)
        progress.setValue(0)

        combined_stats = {}
        overall_winner = None
        total_wins = {}
        failed_files = []
        canceled = False

        executor = concurrent.futures.ProcessPoolExecutor()
        futures = {
            executor.submit(process_file, file_path, self.cache_dir): file_path
            for file_path in file_paths
        }

        try:
            completed_files = 0
            while futures:
                QApplication.processEvents()
                if progress.wasCanceled():
                    canceled = True
                    break

                done, _ = concurrent.futures.wait(
                    list(futures.keys()),
                    timeout=0.1,
                    return_when=concurrent.futures.FIRST_COMPLETED
                )
                if not done:
                    continue

                for future in done:
                    file_path = futures.pop(future)
                    try:
                        result = future.result()
                    except Exception as exc:
                        print(f"Error processing file {file_path}: {str(exc)}")
                        result = None

                    completed_files += 1
                    progress.setValue(completed_files)

                    if result is None:
                        failed_files.append(os.path.basename(file_path))
                        continue

                    self.all_game_data[os.path.basename(file_path)] = result
                    _, overall_stats, winner = result
                    for player, stats in overall_stats.items():
                        if player not in combined_stats:
                            combined_stats[player] = {stat: [] for stat in stats}
                            total_wins[player] = 0
                        for stat, value in stats.items():
                            combined_stats[player][stat].append(value)

                    if winner in total_wins:
                        total_wins[winner] += 1

                    QApplication.processEvents()
                    if progress.wasCanceled():
                        canceled = True
                        break
        finally:
            if canceled:
                for future in futures:
                    future.cancel()
            executor.shutdown(wait=not canceled, cancel_futures=canceled)

        if canceled:
            progress.cancel()
            return
             
        for player in combined_stats:
            for stat in combined_stats[player]:
                combined_stats[player][stat] = sum(combined_stats[player][stat]) / len(combined_stats[player][stat])

        if total_wins:
            overall_winner = max(total_wins, key=total_wins.get)

        progress.setValue(len(file_paths))

        if not combined_stats:
            self.clear_analysis_view()
            if failed_files:
                failed_summary = ", ".join(failed_files[:5])
                if len(failed_files) > 5:
                    failed_summary += ", ..."
                QMessageBox.warning(
                    self,
                    "Analysis Failed",
                    f"Failed to process the selected replay files: {failed_summary}"
                )
            return

        self.set_current_analysis(combined_stats, overall_winner)
        self.apply_current_view()
        self.update_player_profiles(combined_stats)
        self.update_player_profiles_display()

        if failed_files:
            failed_summary = ", ".join(failed_files[:5])
            if len(failed_files) > 5:
                failed_summary += ", ..."
            QMessageBox.warning(
                self,
                "Partial Analysis Complete",
                f"Failed to process {len(failed_files)} file(s): {failed_summary}"
            )
    
    def reprocess_all_files(self):
        if not self.current_folder:
            return

        file_names = sorted(
            file_name for file_name in os.listdir(self.current_folder)
            if file_name.endswith('.ttrm')
        )
        if not file_names:
            QMessageBox.information(self, "No Files", "No replay files were found in the selected folder.")
            return

        progress = QProgressDialog("Rebuilding replay cache...", "Cancel", 0, len(file_names), self)
        progress.setWindowModality(Qt.WindowModal)
        progress.setMinimumDuration(0)
        failed_files = []
        canceled = False

        for index, file_name in enumerate(file_names, start=1):
            QApplication.processEvents()
            if progress.wasCanceled():
                canceled = True
                break

            file_path = os.path.join(self.current_folder, file_name)
            result = process_file(file_path, self.cache_dir, force_reprocess=True)
            if result is not None:
                self.all_game_data[file_name] = result
            else:
                failed_files.append(file_name)

            progress.setValue(index)

        if canceled:
            progress.cancel()
            return

        if self.current_file and self.current_file in self.all_game_data:
            self.display_results(self.current_file)

        if failed_files:
            QMessageBox.warning(
                self,
                "Rebuild Complete",
                f"Rebuilt cache with {len(failed_files)} failure(s)."
            )
        else:
            QMessageBox.information(self, "Rebuild Complete", "Replay cache rebuilt for all files in the current folder.")

if __name__ == "__main__":
    app = QApplication(sys.argv)
    window = ReplayAnalyzer()
    window.show()
    sys.exit(app.exec_())
