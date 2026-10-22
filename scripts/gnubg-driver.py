# gnubg 안에서 도는 수 선택 서버(측정 전용, 배포에 안 씀). engine/examples/vs_gnubg.rs가 띄운다.
#   gnubg -t -q -p scripts/gnubg-driver.py
# 입력 한 줄: "ply <n>" 또는 "<굴릴 쪽 25칸> <상대 25칸> <d1> <d2>"(칸 0..23 = 포인트 1..24, 24 = 바, 각자 기준)
#   또는 "cube <굴릴 쪽 25칸> <상대 25칸> <큐브 값> <소유: -1 가운데, 1 굴릴 쪽, 0 상대>"(굴리기 전 큐브 판단, 머니)
#   → "CUBE <optimal> <no double> <double/take> <double/pass> <추천 번호>"(굴릴 쪽 기준, 큐브 1 정규화 에퀴티)
# 출력: "READY <cubeinfo>", "OK", "MOVE <from> <to> ..."(gnubg findbestmove, 포인트 1..24, 바 25, 베어오프 0)
# 큐브 없는 머니 단판으로 비교하므로 제이코비 규칙을 끄고(켜 두면 큐브가 가운데일 때 개먼 가치가 0이 된다)
# 큐브 없는 평가(cubeful 0)·지정한 깊이를 수마다 직접 넘긴다.
import sys
import gnubg

for c in ['set player 0 human', 'set player 1 human', 'set automatic roll off', 'set automatic game off',
          'set jacoby off', 'new game', 'set turn 1']:
    gnubg.command(c)
plies = 0
print('READY ' + repr(gnubg.cubeinfo()), flush=True)
for line in sys.stdin:
    parts = line.split()
    if not parts:
        continue
    if parts[0] == 'ply':
        plies = int(parts[1])
        print('OK', flush=True)
        continue
    if parts[0] == 'cube':
        v = list(map(int, parts[1:]))
        me, opp, value, owner = v[0:25], v[25:50], v[50], v[51]
        ci = gnubg.cubeinfo(value, owner, 1, 0, (0, 0), 0, 0)
        ci['jacoby'] = 0
        ci = gnubg.calcgammonprice(ci)
        ec = {'cubeful': 1, 'plies': plies, 'deterministic': 1, 'prune': 1, 'noise': 0.0}
        r = gnubg.cfevaluate((opp, me), ci, ec)
        print('CUBE %f %f %f %f %d' % (r[0], r[1], r[2], r[3], r[4]), flush=True)
        continue
    v = list(map(int, parts))
    me, opp, d1, d2 = v[0:25], v[25:50], v[50], v[51]
    gnubg.command('set board %s' % gnubg.positionid((opp, me)))
    gnubg.command('set dice %d%d' % (d1, d2))
    ec = {'cubeful': 0, 'plies': plies, 'deterministic': 1, 'prune': 1, 'noise': 0.0}
    mv = gnubg.findbestmove(gnubg.board(), gnubg.cubeinfo(), ec)
    print('MOVE ' + ' '.join(str(x) for x in mv), flush=True)
